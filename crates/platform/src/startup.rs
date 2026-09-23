//! System startup primitives (pure functions, no syscalls).
//! Windows Service / macOS LaunchDaemon parameter builders,
//! `sc query` state parsing, and elevation command assembly.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StartupMode {
    Boot,
    Login,
    Disabled,
}

pub const WINDOWS_SERVICE_NAME: &str = "easyJobAgent";
pub const WINDOWS_SERVICE_DISPLAY_NAME: &str = "easyJob Agent";
pub const WINDOWS_SYSTEM_PIPE: &str = r"\\.\pipe\easyjob-system";
pub const MACOS_DAEMON_LABEL: &str = "com.easyjob.agent";
pub const MACOS_SYSTEM_DATA_DIR: &str = "/Library/Application Support/EasyJob";
pub const MACOS_DAEMON_PLIST_PATH: &str = "/Library/LaunchDaemons/com.easyjob.agent.plist";

/// Resolve the single system data dir.
///
/// Windows: `<exe_dir>/data` (`exe_dir=None` falls back to current exe dir).
/// macOS: fixed `/Library/Application Support/EasyJob`.
/// Others (incl. Linux): legacy per-user `~/.easyjob`, unchanged.
pub fn system_data_dir(exe_dir: Option<&Path>) -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        if let Some(dir) = exe_dir {
            return dir.join("data");
        }
        if let Ok(exe) = std::env::current_exe() {
            if let Some(parent) = exe.parent() {
                return parent.join("data");
            }
        }
        PathBuf::from("C:/ProgramData/easyJob/data")
    }
    #[cfg(target_os = "macos")]
    {
        let _ = exe_dir;
        PathBuf::from(MACOS_SYSTEM_DATA_DIR)
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        let _ = exe_dir;
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
        PathBuf::from(home).join(".easyjob")
    }
}

/// SCM ImagePath: `"<exe>" --service --data-dir "<dir>"`.
pub fn windows_service_image_path(agent_exe: &Path, data_dir: &Path) -> String {
    format!(
        "\"{}\" --service --data-dir \"{}\"",
        agent_exe.display(),
        data_dir.display()
    )
}

/// Quote one PowerShell argument with single quotes (inner quotes doubled).
pub fn quote_ps_arg(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

/// Elevated install: returns (`powershell.exe`, args) with `-Verb RunAs -Wait`.
pub fn windows_install_elevated_ps(agent_exe: &Path, data_dir: &Path) -> (String, Vec<String>) {
    let script = format!(
        "& {} --install-service --data-dir {}",
        quote_ps_arg(&agent_exe.to_string_lossy()),
        quote_ps_arg(&data_dir.to_string_lossy())
    );
    (
        String::from("powershell.exe"),
        vec![
            String::from("-NoProfile"),
            String::from("-NonInteractive"),
            String::from("-Command"),
            format!(
                "Start-Process powershell.exe -ArgumentList {} -Verb RunAs -Wait",
                quote_ps_arg(&format!(
                    "-NoProfile -NonInteractive -Command {}",
                    script
                ))
            ),
        ],
    )
}

/// Elevated uninstall: returns (`powershell.exe`, args) with `-Verb RunAs -Wait`.
pub fn windows_uninstall_elevated_ps(agent_exe: &Path) -> (String, Vec<String>) {
    let script = format!(
        "& {} --uninstall-service",
        quote_ps_arg(&agent_exe.to_string_lossy())
    );
    (
        String::from("powershell.exe"),
        vec![
            String::from("-NoProfile"),
            String::from("-NonInteractive"),
            String::from("-Command"),
            format!(
                "Start-Process powershell.exe -ArgumentList {} -Verb RunAs -Wait",
                quote_ps_arg(&format!(
                    "-NoProfile -NonInteractive -Command {}",
                    script
                ))
            ),
        ],
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowsServiceState {
    Running,
    Stopped,
    NotInstalled,
}

/// Parse `sc query` / `sc qc` style output.
///
/// `RUNNING` wins; service-missing markers (1060 / does-not-exist / 未安装) map
/// to NotInstalled; anything else maps to Stopped.
pub fn parse_sc_query_state(output: &str) -> WindowsServiceState {
    let upper = output.to_uppercase();
    if upper.contains("RUNNING") {
        return WindowsServiceState::Running;
    }
    if output.contains("1060")
        || upper.contains("DOES NOT EXIST")
        || upper.contains("DOESN'T EXIST")
        || output.contains("未安装")
    {
        return WindowsServiceState::NotInstalled;
    }
    WindowsServiceState::Stopped
}

/// macOS LaunchDaemon plist XML with RunAtLoad + KeepAlive and log redirection.
pub fn macos_daemon_plist(agent_exe: &Path, data_dir: &Path) -> String {
    let logs = data_dir.join("logs");
    format!(
        concat!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n",
            "<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" ",
            "\"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n",
            "<plist version=\"1.0\">\n",
            "<dict>\n",
            "    <key>Label</key>\n",
            "    <string>{label}</string>\n",
            "    <key>ProgramArguments</key>\n",
            "    <array>\n",
            "        <string>{exe}</string>\n",
            "        <string>--daemon</string>\n",
            "        <string>--data-dir</string>\n",
            "        <string>{data}</string>\n",
            "    </array>\n",
            "    <key>RunAtLoad</key>\n",
            "    <true/>\n",
            "    <key>KeepAlive</key>\n",
            "    <true/>\n",
            "    <key>StandardOutPath</key>\n",
            "    <string>{out_log}</string>\n",
            "    <key>StandardErrorPath</key>\n",
            "    <string>{err_log}</string>\n",
            "</dict>\n",
            "</plist>\n",
        ),
        label = MACOS_DAEMON_LABEL,
        exe = agent_exe.display(),
        data = data_dir.display(),
        out_log = logs.join("daemon.out.log").display(),
        err_log = logs.join("daemon.err.log").display(),
    )
}

/// Shell script executed via `osascript ... with administrator privileges`.
///
/// Writes the plist, fixes ownership/permissions, then bootstraps the daemon.
pub fn macos_install_script(plist_path: &str, plist_content: &str) -> String {
    format!(
        concat!(
            "set -e\n",
            "PLIST_PATH={plist_q}\n",
            "mkdir -p /Library/LaunchDaemons\n",
            "cat > \"$PLIST_PATH\" <<'EASYJOB_PLIST_EOF'\n",
            "{content}\n",
            "EASYJOB_PLIST_EOF\n",
            "chown root:wheel \"$PLIST_PATH\"\n",
            "chmod 644 \"$PLIST_PATH\"\n",
            "/bin/launchctl bootout system/\"$PLIST_PATH\" 2>/dev/null || true\n",
            "/bin/launchctl bootstrap system/ \"$PLIST_PATH\"\n",
        ),
        plist_q = sh_single_quote(plist_path),
        content = plist_content.trim_end(),
    )
}

/// macOS uninstall script for `osascript ... with administrator privileges`.
pub fn macos_uninstall_script(plist_path: &str) -> String {
    format!(
        concat!(
            "set -e\n",
            "PLIST_PATH={plist_q}\n",
            "/bin/launchctl bootout system/\"$PLIST_PATH\" 2>/dev/null || true\n",
            "rm -f \"$PLIST_PATH\"\n",
        ),
        plist_q = sh_single_quote(plist_path),
    )
}

fn sh_single_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\"'\"'"))
}
