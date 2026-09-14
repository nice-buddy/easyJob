use std::path::PathBuf;

pub const MAX_FRAME_LENGTH: usize = 4 * 1024 * 1024; // 4MB

pub fn default_ipc_path() -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        let username = std::env::var("USERNAME").unwrap_or_else(|_| "default".to_string());
        PathBuf::from(format!(r"\\.\pipe\easyjob-{}", username))
    }
    #[cfg(not(target_os = "windows"))]
    {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
        PathBuf::from(home).join(".easyjob").join("easyjob.sock")
    }
}
