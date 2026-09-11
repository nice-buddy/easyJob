#[cfg(unix)]
use std::os::unix::process::CommandExt;
#[cfg(unix)]
use std::process::Command as StdCommand;

#[cfg(unix)]
pub fn configure_unix_command(cmd: &mut StdCommand) {
    // Set child as process group leader
    unsafe {
        cmd.pre_exec(|| {
            libc::setpgid(0, 0);
            Ok(())
        });
    }
}

#[cfg(unix)]
pub async fn kill_unix_process_tree(pid: u32) -> easyjob_common::Result<()> {
    if pid <= 1 || pid > i32::MAX as u32 {
        return Err(easyjob_common::Error::Process(format!(
            "Refusing to kill system or invalid process with PID {pid}"
        )));
    }
    unsafe {
        let pgid = pid as i32;
        // Graceful termination: send SIGTERM to process group and direct pid
        let _ = libc::kill(-pgid, libc::SIGTERM);
        let _ = libc::kill(pgid, libc::SIGTERM);
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        // Force kill: send SIGKILL to process group and direct pid
        let _ = libc::kill(-pgid, libc::SIGKILL);
        let _ = libc::kill(pgid, libc::SIGKILL);
    }
    Ok(())
}
