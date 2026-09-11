#[cfg(windows)]
use std::os::windows::process::CommandExt;
#[cfg(windows)]
use std::process::Command as StdCommand;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x08000000;

#[cfg(windows)]
pub fn configure_windows_command(cmd: &mut StdCommand) {
    cmd.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(windows)]
pub async fn kill_windows_process_tree(pid: u32) -> easyjob_common::Result<()> {
    if pid == 0 {
        return Err(easyjob_common::Error::Process(
            "Invalid PID 0 for process tree kill".to_string(),
        ));
    }
    // taskkill /F /T /PID <pid>
    let _ = tokio::process::Command::new("taskkill")
        .args(["/F", "/T", "/PID", &pid.to_string()])
        .output()
        .await;
    Ok(())
}
