//! 系统服务管理：查询 / 安装 / 卸载开机自启载体。
//!
//! - Windows: Windows Service（`sc.exe` 查询，提权拉起 agent `--install-service`）
//! - macOS: LaunchDaemon（plist 存在性 + `launchctl print`，提权写 plist 并 bootstrap）
//! 登录项侧继续由前端 `tauri-plugin-autostart` 管理，本模块只管 service 侧。

#[cfg(target_os = "windows")]
use easyjob_platform::startup::{
    parse_sc_query_state, windows_install_elevated_ps, windows_uninstall_elevated_ps,
    WindowsServiceState,
};
#[cfg(target_os = "macos")]
use easyjob_platform::startup::{MACOS_DAEMON_LABEL, MACOS_DAEMON_PLIST_PATH};
use serde::Serialize;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize)]
pub struct ServiceStatus {
    pub installed: bool,
    pub running: bool,
    pub data_dir: Option<String>,
}

/// 供测试与启动时快速失败兜底的同步版本：从不执行提权，只做本地查询。
pub fn service_status_sync_fallback() -> ServiceStatus {
    query_service_status().unwrap_or(ServiceStatus {
        installed: false,
        running: false,
        data_dir: None,
    })
}

fn query_service_status() -> Result<ServiceStatus, String> {
    #[cfg(target_os = "windows")]
    {
        let out = std::process::Command::new("sc.exe")
            .args(["query", easyjob_platform::startup::WINDOWS_SERVICE_NAME])
            .output()
            .map_err(|e| format!("查询系统服务失败：{e}"))?;
        let text = format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        let state = parse_sc_query_state(&text);
        let data_dir = query_windows_service_data_dir().ok().flatten();
        Ok(match state {
            WindowsServiceState::Running => ServiceStatus {
                installed: true,
                running: true,
                data_dir,
            },
            WindowsServiceState::Stopped => ServiceStatus {
                installed: true,
                running: false,
                data_dir,
            },
            WindowsServiceState::NotInstalled => ServiceStatus {
                installed: false,
                running: false,
                data_dir: None,
            },
        })
    }
    #[cfg(target_os = "macos")]
    {
        let plist_exists = std::path::Path::new(MACOS_DAEMON_PLIST_PATH).exists();
        let print_ok = std::process::Command::new("/bin/launchctl")
            .args(["print", &format!("system/{MACOS_DAEMON_LABEL}")])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        Ok(ServiceStatus {
            installed: plist_exists,
            running: plist_exists && print_ok,
            data_dir: Some(
                easyjob_platform::startup::MACOS_SYSTEM_DATA_DIR.to_string(),
            ),
        })
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        Ok(ServiceStatus {
            installed: false,
            running: false,
            data_dir: None,
        })
    }
}

/// 从服务注册的 ImagePath 里解析 `--data-dir`（Windows）。
#[cfg(target_os = "windows")]
fn query_windows_service_data_dir() -> Result<Option<String>, String> {
    let out = std::process::Command::new("sc.exe")
        .args([
            "qc",
            easyjob_platform::startup::WINDOWS_SERVICE_NAME,
        ])
        .output()
        .map_err(|e| format!("查询服务配置失败：{e}"))?;
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    Ok(parse_data_dir_from_qc(&text))
}

#[cfg(target_os = "windows")]
fn parse_data_dir_from_qc(text: &str) -> Option<String> {
    let marker = "--data-dir";
    let idx = text.find(marker)?;
    let rest = text[idx + marker.len()..].trim_start();
    let rest = rest.strip_prefix('"').unwrap_or(rest);
    // Windows 路径不含引号包裹时的终止符为空格；含引号时到下一个引号。
    let quoted = text[idx + marker.len()..].trim_start().starts_with('"');
    if quoted {
        rest.split('"').next().map(|s| s.to_string())
    } else {
        rest.split_whitespace().next().map(|s| s.to_string())
    }
}

fn find_agent_binary() -> Result<PathBuf, String> {
    #[cfg(target_os = "windows")]
    let bin_name = "easyjob-agent.exe";
    #[cfg(not(target_os = "windows"))]
    let bin_name = "easyjob-agent";

    if let Ok(current_exe) = std::env::current_exe() {
        let mut dir = current_exe;
        dir.pop();
        let target_bin = dir.join(bin_name);
        if target_bin.exists() {
            return Ok(target_bin);
        }
        let resources_bin = dir.join("../Resources").join(bin_name);
        if resources_bin.exists() {
            return Ok(resources_bin);
        }
    }
    Ok(PathBuf::from(bin_name))
}

#[tauri::command]
pub async fn service_status() -> Result<ServiceStatus, String> {
    query_service_status()
}

#[tauri::command]
pub async fn service_install() -> Result<ServiceStatus, String> {
    install_service_impl()?;
    // 轮询等待服务进入运行态（最多约 15s）。
    for _ in 0..30 {
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        match query_service_status() {
            Ok(st) if st.running => return Ok(st),
            Ok(_) => {}
            Err(e) => return Err(e),
        }
    }
    Err("系统服务安装成功，但未能在 15 秒内进入运行状态，请检查系统服务管理器".to_string())
}

#[tauri::command]
pub async fn service_uninstall() -> Result<ServiceStatus, String> {
    uninstall_service_impl()?;
    Ok(query_service_status().unwrap_or(ServiceStatus {
        installed: false,
        running: false,
        data_dir: None,
    }))
}

fn install_service_impl() -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        let agent = find_agent_binary()?;
        let exe_dir = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.to_path_buf()));
        let data_dir = easyjob_platform::startup::system_data_dir(exe_dir.as_deref());
        std::fs::create_dir_all(&data_dir).map_err(|e| format!("创建系统数据目录失败：{e}"))?;
        let (program, args) = windows_install_elevated_ps(&agent, &data_dir);
        run_elevated(&program, &args, "安装系统服务")
    }
    #[cfg(target_os = "macos")]
    {
        use easyjob_platform::startup::{macos_daemon_plist, macos_install_script};
        let agent = find_agent_binary()?;
        let data_dir =
            PathBuf::from(easyjob_platform::startup::MACOS_SYSTEM_DATA_DIR);
        let plist = macos_daemon_plist(&agent, &data_dir);
        let script = macos_install_script(MACOS_DAEMON_PLIST_PATH, &plist);
        run_macos_admin(&script, "安装开机自启服务")
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        Err("当前平台不支持开机自启".to_string())
    }
}

fn uninstall_service_impl() -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        // 未安装时视为成功，保证切换幂等。
        match query_service_status() {
            Ok(st) if !st.installed => return Ok(()),
            _ => {}
        }
        let agent = find_agent_binary()?;
        let (program, args) = windows_uninstall_elevated_ps(&agent);
        run_elevated(&program, &args, "卸载系统服务")
    }
    #[cfg(target_os = "macos")]
    {
        use easyjob_platform::startup::macos_uninstall_script;
        if !std::path::Path::new(MACOS_DAEMON_PLIST_PATH).exists() {
            return Ok(());
        }
        let script = macos_uninstall_script(MACOS_DAEMON_PLIST_PATH);
        run_macos_admin(&script, "卸载开机自启服务")
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        Err("当前平台不支持开机自启".to_string())
    }
}

#[cfg(target_os = "windows")]
fn run_elevated(program: &str, args: &[String], action: &str) -> Result<(), String> {
    let out = std::process::Command::new(program)
        .args(args)
        .output()
        .map_err(|e| format!("{action}失败：{e}"))?;
    if out.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&out.stderr);
    if stderr.contains("cancelled") || stderr.contains("denied") {
        return Err(format!("{action}已取消（用户拒绝提权）"));
    }
    Err(format!(
        "{action}失败：{}",
        stderr.trim().to_string().chars().take(300).collect::<String>()
    ))
}

#[cfg(target_os = "macos")]
fn run_macos_admin(script: &str, action: &str) -> Result<(), String> {
    let out = std::process::Command::new("/usr/bin/osascript")
        .args([
            "-e",
            &format!(
                "do shell script {} with administrator privileges",
                sh_quote(script)
            ),
        ])
        .output()
        .map_err(|e| format!("{action}失败：{e}"))?;
    if out.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&out.stderr);
    if stderr.contains("User canceled") || out.status.code() == Some(1) && stderr.trim().is_empty() {
        return Err(format!("{action}已取消（用户拒绝提权）"));
    }
    Err(format!(
        "{action}失败：{}",
        stderr.trim().to_string().chars().take(300).collect::<String>()
    ))
}

#[cfg(target_os = "macos")]
fn sh_quote(s: &str) -> String {
    let mut out = String::from("\"");
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            _ => out.push(ch),
        }
    }
    out.push('"');
    out
}
