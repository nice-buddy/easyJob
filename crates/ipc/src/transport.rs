use std::path::{Path, PathBuf};

pub const MAX_FRAME_LENGTH: usize = 4 * 1024 * 1024; // 4MB

/// Windows: fixed system pipe, shared by the service and all desktop sessions.
/// macOS: socket under the fixed system data dir.
pub fn system_ipc_path() -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        PathBuf::from(r"\\.\pipe\easyjob-system")
    }
    #[cfg(not(target_os = "windows"))]
    {
        system_data_dir_fallback().join("easyjob.sock")
    }
}

/// Unix socket follows the data dir. Windows pipe name is fixed and ignores `data_dir`.
pub fn ipc_path_for_data_dir(data_dir: &Path) -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        let _ = data_dir;
        PathBuf::from(r"\\.\pipe\easyjob-system")
    }
    #[cfg(not(target_os = "windows"))]
    {
        data_dir.join("easyjob.sock")
    }
}

#[cfg(not(target_os = "windows"))]
fn system_data_dir_fallback() -> PathBuf {
    #[cfg(target_os = "macos")]
    {
        PathBuf::from("/Library/Application Support/EasyJob")
    }
    #[cfg(not(target_os = "macos"))]
    {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
        PathBuf::from(home).join(".easyjob")
    }
}

/// Back-compat entry point: now resolves to the system channel.
pub fn default_ipc_path() -> PathBuf {
    system_ipc_path()
}
