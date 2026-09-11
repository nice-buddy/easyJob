use easyjob_common::Result;
use tokio::process::Command;

pub struct CommandBuilder;

impl CommandBuilder {
    pub fn new_shell(script: &str) -> Command {
        #[cfg(windows)]
        {
            let mut cmd = Command::new("cmd.exe");
            cmd.args(["/c", script]);
            crate::windows::configure_windows_command(cmd.as_std_mut());
            cmd
        }
        #[cfg(unix)]
        {
            let shell = if std::path::Path::new("/bin/zsh").exists() {
                "/bin/zsh"
            } else {
                "/bin/sh"
            };
            let mut cmd = Command::new(shell);
            cmd.args(["-c", script]);
            crate::unix::configure_unix_command(cmd.as_std_mut());
            cmd
        }
        #[cfg(not(any(unix, windows)))]
        {
            let mut cmd = Command::new("sh");
            cmd.args(["-c", script]);
            cmd
        }
    }

    pub fn new_cmd(command: &str) -> Command {
        let mut cmd = Command::new("cmd.exe");
        cmd.args(["/c", command]);
        #[cfg(windows)]
        crate::windows::configure_windows_command(cmd.as_std_mut());
        #[cfg(unix)]
        crate::unix::configure_unix_command(cmd.as_std_mut());
        cmd
    }

    pub fn new_powershell(script: &str, no_profile: bool) -> Command {
        let mut cmd = Command::new("powershell.exe");
        if no_profile {
            cmd.arg("-NoProfile");
        }
        cmd.args([
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            script,
        ]);
        #[cfg(windows)]
        crate::windows::configure_windows_command(cmd.as_std_mut());
        #[cfg(unix)]
        crate::unix::configure_unix_command(cmd.as_std_mut());
        cmd
    }

    pub fn new_program(program: &str, args: &[String]) -> Command {
        let mut cmd = Command::new(program);
        cmd.args(args);
        #[cfg(windows)]
        crate::windows::configure_windows_command(cmd.as_std_mut());
        #[cfg(unix)]
        crate::unix::configure_unix_command(cmd.as_std_mut());
        cmd
    }
}

pub async fn kill_process_tree(pid: u32) -> Result<()> {
    #[cfg(unix)]
    {
        crate::unix::kill_unix_process_tree(pid).await
    }
    #[cfg(windows)]
    {
        crate::windows::kill_windows_process_tree(pid).await
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = pid;
        Err(easyjob_common::Error::Process(
            "Unsupported platform for kill_process_tree".to_string(),
        ))
    }
}

/// Placeholder for platform-specific process metadata or handle if needed
pub struct PlatformProcess;
