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
            // macOS/Windows 的系统数据目录使用共享通道，登录后的桌面端以普通用户身份连接。
            // 其他平台保持原有的私有 socket 行为，避免权限回退。
            let shared = cfg!(any(target_os = "macos", target_os = "windows"));
            let service =
                AgentService::init_shared(&db_url, &ipc_path, cli.max_concurrent, shared).await?;

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

#[cfg(not(target_os = "windows"))]
fn install_service_cmd(_data_dir: Option<PathBuf>) -> Result<(), Box<dyn std::error::Error>> {
    Err("`--install-service` 仅在 Windows 上支持".into())
}

#[cfg(target_os = "windows")]
fn install_service_cmd(data_dir: Option<PathBuf>) -> Result<(), Box<dyn std::error::Error>> {
    use easyjob_platform::startup::{
        windows_service_image_path, WindowsServiceState, WINDOWS_SERVICE_DISPLAY_NAME,
        WINDOWS_SERVICE_NAME,
    };
    let data_dir = resolve_data_dir(data_dir);
    std::fs::create_dir_all(data_dir.join("logs"))?;
    grant_authenticated_users_modify(&data_dir)?;
    let agent_exe = std::env::current_exe()?;
    let image = windows_service_image_path(&agent_exe, &data_dir);
    let state = sc_query_state()?;
    if state == WindowsServiceState::NotInstalled {
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
    } else {
        run_elevated_sc(&[
            "config",
            WINDOWS_SERVICE_NAME,
            &format!("binPath= {image}"),
            "start=",
            "auto",
            "obj=",
            "LocalSystem",
        ])?;
    }
    // 失败自动重启：5 秒后重启服务。
    let _ = run_elevated_sc(&[
        "failure",
        WINDOWS_SERVICE_NAME,
        "reset=",
        "0",
        "actions=",
        "restart/5000",
    ]);
    // 已在运行时 sc start 会报错，视为成功。
    let _ = run_elevated_sc(&["start", WINDOWS_SERVICE_NAME]);
    if sc_query_state()? != WindowsServiceState::Running {
        return Err("Windows 服务已安装，但未能进入运行状态".into());
    }
    println!("Windows 服务已安装并启动。");
    Ok(())
}

#[cfg(not(target_os = "windows"))]
fn uninstall_service_cmd() -> Result<(), Box<dyn std::error::Error>> {
    Err("`--uninstall-service` 仅在 Windows 上支持".into())
}

#[cfg(target_os = "windows")]
fn uninstall_service_cmd() -> Result<(), Box<dyn std::error::Error>> {
    use easyjob_platform::startup::{WindowsServiceState, WINDOWS_SERVICE_NAME};
    if sc_query_state()? == WindowsServiceState::NotInstalled {
        return Ok(());
    }
    let _ = run_elevated_sc(&["stop", WINDOWS_SERVICE_NAME]);
    for _ in 0..20 {
        if sc_query_state()? == WindowsServiceState::Stopped {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(500));
    }
    run_elevated_sc(&["delete", WINDOWS_SERVICE_NAME])?;
    println!("Windows 服务已卸载。");
    Ok(())
}

#[cfg(target_os = "windows")]
fn sc_query_state(
) -> Result<easyjob_platform::startup::WindowsServiceState, Box<dyn std::error::Error>> {
    use easyjob_platform::startup::{parse_sc_query_state, WINDOWS_SERVICE_NAME};
    let out = run_sc(&["query", WINDOWS_SERVICE_NAME])?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    Ok(parse_sc_query_state(&text))
}

#[cfg(target_os = "windows")]
fn run_sc(args: &[&str]) -> Result<std::process::Output, Box<dyn std::error::Error>> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x08000000;
    let mut cmd = std::process::Command::new("sc.exe");
    cmd.args(args);
    cmd.creation_flags(CREATE_NO_WINDOW);
    Ok(cmd.output()?)
}

#[cfg(target_os = "windows")]
fn run_elevated_sc(args: &[&str]) -> Result<(), Box<dyn std::error::Error>> {
    let out = run_sc(args)?;
    if out.status.success() {
        return Ok(());
    }
    Err(format!(
        "sc.exe {} 执行失败：{} {}",
        args.join(" "),
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
    .into())
}

/// 授予 Authenticated Users 对系统数据目录的修改权限，登录用户才能连上服务并读写数据。
#[cfg(target_os = "windows")]
fn grant_authenticated_users_modify(
    data_dir: &std::path::Path,
) -> Result<(), Box<dyn std::error::Error>> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x08000000;
    let out = std::process::Command::new("icacls")
        .arg(data_dir)
        .args(["/grant", "*S-1-5-11:(OI)(CI)M", "/T", "/C"])
        .creation_flags(CREATE_NO_WINDOW)
        .output()?;
    if out.status.success() {
        return Ok(());
    }
    Err(format!("icacls 授权失败：{}", String::from_utf8_lossy(&out.stdout)).into())
}

#[cfg(target_os = "windows")]
struct WindowsServiceConfig {
    db_url: String,
    ipc_path: PathBuf,
    max_concurrent: usize,
}

#[cfg(target_os = "windows")]
static WINDOWS_SERVICE_CONFIG: std::sync::OnceLock<WindowsServiceConfig> =
    std::sync::OnceLock::new();

#[cfg(target_os = "windows")]
windows_service::define_windows_service!(ffi_service_main, service_main_inner);

#[cfg(target_os = "windows")]
fn service_main_inner(_args: Vec<std::ffi::OsString>) {
    use easyjob_platform::startup::WINDOWS_SERVICE_NAME;
    use windows_service::{
        service::{
            ServiceControl, ServiceControlAccept, ServiceExitCode, ServiceState, ServiceStatus,
            ServiceType,
        },
        service_control_handler::{self, ServiceControlHandlerResult},
    };

    let Some(config) = WINDOWS_SERVICE_CONFIG.get() else {
        eprintln!("Windows 服务配置缺失，无法启动");
        return;
    };
    let db_url = config.db_url.clone();
    let ipc_path = config.ipc_path.clone();
    let max_concurrent = config.max_concurrent;

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
        let lock_path = ipc_path
            .parent()
            .map(|d| d.join("agent.lock"))
            .unwrap_or_else(|| PathBuf::from("agent.lock"));
        let Ok(outcome) = SingleInstanceLock::acquire(&lock_path, &ipc_path).await else {
            return;
        };
        let LockOutcome::Acquired(_lock) = outcome else {
            // 已有实例在跑：不重复启动，直接退出由 SCM 处理。
            return;
        };
        let service =
            match AgentService::init_shared(&db_url, &ipc_path, max_concurrent, true).await {
                Ok(service) => service,
                Err(e) => {
                    eprintln!("AgentService 初始化失败：{e}");
                    return;
                }
            };
        let shutdown = service.shutdown_notify();
        let status_handle = match service_control_handler::register(
            WINDOWS_SERVICE_NAME,
            move |event| match event {
                ServiceControl::Stop | ServiceControl::Shutdown => {
                    shutdown.notify_one();
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
        set_state(ServiceState::Running);
        if let Err(e) = service.run().await {
            eprintln!("Agent 服务运行失败：{e}");
        }
        set_state(ServiceState::Stopped);
    });
}

#[cfg(target_os = "windows")]
fn run_as_windows_service(
    db_url: &str,
    ipc_path: &std::path::Path,
    max_concurrent: usize,
) -> Result<(), Box<dyn std::error::Error>> {
    use easyjob_platform::startup::WINDOWS_SERVICE_NAME;
    let _ = WINDOWS_SERVICE_CONFIG.set(WindowsServiceConfig {
        db_url: db_url.to_string(),
        ipc_path: ipc_path.to_path_buf(),
        max_concurrent,
    });
    windows_service::service_dispatcher::start(WINDOWS_SERVICE_NAME, ffi_service_main)?;
    Ok(())
}
