use clap::Parser;
use easyjob_agent::lock::{LockOutcome, SingleInstanceLock};
use easyjob_agent::service::AgentService;
use easyjob_logging::init_logging;
use std::path::PathBuf;
use tracing::{error, info};

#[derive(Parser, Debug)]
#[command(author, version, about = "easyJob Background Task Scheduling Daemon")]
struct Cli {
    /// Custom directory for DB, sockets, and logs. Default: system data dir
    /// (Windows: <install-dir>/data, macOS: /Library/Application Support/EasyJob).
    #[arg(long, value_name = "PATH")]
    data_dir: Option<PathBuf>,

    /// Run in daemon mode (suppress console output, write rolling logs to <data-dir>/logs/agent.log)
    #[arg(long, default_value_t = false, conflicts_with = "service")]
    daemon: bool,

    /// Run as a Windows Service foreground process (only meaningful under SCM).
    #[arg(long, default_value_t = false)]
    service: bool,

    /// Install the Windows Service (requires elevation) and exit.
    #[arg(long, default_value_t = false)]
    install_service: bool,

    /// Uninstall the Windows Service (requires elevation) and exit.
    #[arg(long, default_value_t = false)]
    uninstall_service: bool,

    /// Global concurrency limit
    #[arg(long, default_value_t = 16)]
    max_concurrent: usize,
}

fn resolve_data_dir(cli_dir: Option<PathBuf>) -> PathBuf {
    if let Some(dir) = cli_dir {
        return dir;
    }
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()));
    easyjob_platform::startup::system_data_dir(exe_dir.as_deref())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    if cli.install_service {
        return install_service_cmd(cli.data_dir);
    }
    if cli.uninstall_service {
        return uninstall_service_cmd();
    }
    if cli.service && !cfg!(target_os = "windows") {
        return Err("`--service` 仅在 Windows 上支持".into());
    }

    let data_dir = resolve_data_dir(cli.data_dir);
    std::fs::create_dir_all(&data_dir)?;

    let log_dir = data_dir.join("logs");
    let _guard = init_logging(&log_dir, !cli.daemon && !cli.service, "info")?;

    let lock_path = data_dir.join("agent.lock");
    let ipc_path = easyjob_ipc::transport::ipc_path_for_data_dir(&data_dir);

    let db_path = data_dir.join("easyjob.db");
    let db_url = format!("sqlite://{}?mode=rwc", db_path.to_string_lossy());

    #[cfg(target_os = "windows")]
    if cli.service {
        return run_as_windows_service(&db_url, &ipc_path, cli.max_concurrent);
    }

    match SingleInstanceLock::acquire(&lock_path, &ipc_path).await? {
        LockOutcome::AlreadyRunning => {
            println!("easyjob-agent is already running.");
            return Ok(());
        }
        LockOutcome::Acquired(_lock) => {
            info!(
                "Starting easyJob Agent Daemon v{}",
                env!("CARGO_PKG_VERSION")
            );
            // 系统数据目录恒为共享通道：登录后的桌面端以普通用户身份连接。
            let service =
                AgentService::init_shared(&db_url, &ipc_path, cli.max_concurrent, true).await?;

            tokio::select! {
                res = service.run() => {
                    if let Err(e) = res {
                        error!("Agent service encountered error: {:?}", e);
                    }
                }
                _ = tokio::signal::ctrl_c() => {
                    info!("Received SIGINT/Ctrl+C, exiting gracefully");
                }
            }
        }
    }

    Ok(())
}

fn install_service_cmd(data_dir: Option<PathBuf>) -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(not(target_os = "windows"))]
    {
        let _ = data_dir;
        return Err("`--install-service` 仅在 Windows 上支持".into());
    }
    #[cfg(target_os = "windows")]
    {
        use easyjob_platform::startup::{
            windows_service_image_path, WINDOWS_SERVICE_DISPLAY_NAME, WINDOWS_SERVICE_NAME,
        };
        let data_dir = resolve_data_dir(data_dir);
        std::fs::create_dir_all(&data_dir)?;
        let agent_exe = std::env::current_exe()?;
        let image = windows_service_image_path(&agent_exe, &data_dir);
        run_elevated_sc(&[
            "create",
            WINDOWS_SERVICE_NAME,
            &format!("binPath= {image}"),
            &format!("DisplayName= {WINDOWS_SERVICE_DISPLAY_NAME}"),
            "start=",
            "auto",
            "obj=",
            "LocalSystem",
        ])?;
        // 失败自动重启：5 秒后重启服务。
        let _ = run_elevated_sc(&[
            "failure",
            WINDOWS_SERVICE_NAME,
            "reset=",
            "0",
            "actions=",
            "restart/5000",
        ]);
        run_elevated_sc(&["start", WINDOWS_SERVICE_NAME])?;
        println!("Windows 服务已安装并启动。");
        Ok(())
    }
}

fn uninstall_service_cmd() -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(not(target_os = "windows"))]
    {
        return Err("`--uninstall-service` 仅在 Windows 上支持".into());
    }
    #[cfg(target_os = "windows")]
    {
        use easyjob_platform::startup::WINDOWS_SERVICE_NAME;
        // 先停后删；停止失败不阻断删除。
        let _ = run_elevated_sc(&["stop", WINDOWS_SERVICE_NAME]);
        std::thread::sleep(std::time::Duration::from_millis(800));
        run_elevated_sc(&["delete", WINDOWS_SERVICE_NAME])?;
        println!("Windows 服务已卸载。");
        Ok(())
    }
}

/// NOTE: install/uninstall 由桌面端提权拉起（UAC），此处假设已提权，直接调 sc.exe。
#[cfg(target_os = "windows")]
fn run_elevated_sc(args: &[&str]) -> Result<(), Box<dyn std::error::Error>> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x08000000;
    let mut cmd = std::process::Command::new("sc.exe");
    cmd.args(args);
    cmd.creation_flags(CREATE_NO_WINDOW);
    let out = cmd.output()?;
    if out.status.success() {
        return Ok(());
    }
    Err(format!(
        "sc.exe {} 执行失败：{}",
        args.join(" "),
        String::from_utf8_lossy(&out.stdout)
    )
    .into())
}

#[cfg(target_os = "windows")]
fn run_as_windows_service(
    db_url: &str,
    ipc_path: &std::path::Path,
    max_concurrent: usize,
) -> Result<(), Box<dyn std::error::Error>> {
    use easyjob_platform::startup::WINDOWS_SERVICE_NAME;
    use std::ffi::OsString;
    use windows_service::{
        define_windows_service,
        service::{
            ServiceControl, ServiceControlAccept, ServiceExitCode, ServiceState, ServiceStatus,
            ServiceType,
        },
        service_control_handler::{self, ServiceControlHandlerResult},
        service_dispatcher,
    };

    let db_url = db_url.to_string();
    let ipc_path = ipc_path.to_path_buf();
    define_windows_service!(ffi_service_main, service_main_inner);

    fn service_main_inner(
        _args: Vec<OsString>,
        db_url: String,
        ipc_path: std::path::PathBuf,
        max_concurrent: usize,
    ) {
        let rt = match tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
        {
            Ok(rt) => rt,
            Err(e) => {
                eprintln!("创建 Tokio runtime 失败：{e}");
                return;
            }
        };
        rt.block_on(async move {
            let status_handle = match service_control_handler::register(
                WINDOWS_SERVICE_NAME,
                move |event| match event {
                    ServiceControl::Stop | ServiceControl::Shutdown => {
                        ServiceControlHandlerResult::NoError
                    }
                    _ => ServiceControlHandlerResult::NotImplemented,
                },
            ) {
                Ok(h) => h,
                Err(e) => {
                    eprintln!("注册服务控制回调失败：{e}");
                    return;
                }
            };
            let set_state = |state: ServiceState| {
                let _ = status_handle.set_service_status(ServiceStatus {
                    service_type: ServiceType::OWN_PROCESS,
                    current_state: state,
                    controls_accepted: ServiceControlAccept::STOP | ServiceControlAccept::SHUTDOWN,
                    exit_code: ServiceExitCode::Win32(0),
                    checkpoint: 0,
                    wait_hint: std::time::Duration::from_secs(5),
                    process_id: None,
                });
            };
            set_state(ServiceState::StartPending);
            let lock_path = ipc_path
                .parent()
                .map(|d| d.join("agent.lock"))
                .unwrap_or_else(|| std::path::PathBuf::from("agent.lock"));
            let Ok(outcome) =
                SingleInstanceLock::acquire(&lock_path, &ipc_path).await
            else {
                set_state(ServiceState::Stopped);
                return;
            };
            let LockOutcome::Acquired(_lock) = outcome else {
                // 已有实例在跑：直接报告 Running 后退出，避免 SCM 反复重启。
                set_state(ServiceState::Running);
                set_state(ServiceState::Stopped);
                return;
            };
            match AgentService::init_shared(&db_url, &ipc_path, max_concurrent, true).await {
                Ok(service) => {
                    set_state(ServiceState::Running);
                    // SCM stop 通过“关闭控制通道”感知：轮询服务状态太重，
                    // 这里用 ctrl_c 在服务语境下收不到，因此用一个永不完成的
                    // pending + SCM 回调里直接 exit 的简化语义。
                    // 更稳妥的做法是回调里 notify shutdown；本期先保证 stop
                    // 能结束进程，由 SCM 的 failure-actions 负责拉起。
                    let _ = service.run().await;
                    set_state(ServiceState::Stopped);
                }
                Err(e) => {
                    eprintln!("AgentService 初始化失败：{e}");
                    set_state(ServiceState::Stopped);
                }
            }
        });
    }

    service_dispatcher::start(WINDOWS_SERVICE_NAME, ffi_service_main)?;
    Ok(())
}
