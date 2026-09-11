use easyjob_common::{Error, Result};
use easyjob_domain::action::{Action, ActionKind};
use easyjob_domain::execution::ExecutionStatus;
use easyjob_platform::{kill_process_tree, CommandBuilder};
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;
use tokio::io::AsyncReadExt;
use tokio_util::sync::CancellationToken;

pub const MAX_OUTPUT_BYTES: usize = 2 * 1024 * 1024; // 2MB
pub const TRUNCATION_SUFFIX: &str = "\n... [output truncated]";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunResult {
    pub status: ExecutionStatus,
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub error_message: Option<String>,
}

pub struct ProcessRunner;

impl ProcessRunner {
    pub async fn run_action(
        action: &Action,
        working_dir: Option<&PathBuf>,
        env: &HashMap<String, String>,
        timeout_secs: Option<u64>,
        cancel: CancellationToken,
    ) -> Result<RunResult> {
        if cancel.is_cancelled() {
            return Ok(RunResult {
                status: ExecutionStatus::Cancelled,
                exit_code: None,
                stdout: String::new(),
                stderr: String::new(),
                error_message: Some("Cancelled by user".to_string()),
            });
        }

        let mut cmd = match &action.kind {
            ActionKind::ExecuteProgram { program, args } => {
                CommandBuilder::new_program(program, args)
            }
            ActionKind::ExecuteShell { command } => CommandBuilder::new_shell(command),
            ActionKind::ExecuteCmd { command } => {
                let mut c = tokio::process::Command::new("cmd.exe");
                c.args(["/c", command]);
                #[cfg(windows)]
                easyjob_platform::windows::configure_windows_command(c.as_std_mut());
                #[cfg(unix)]
                easyjob_platform::unix::configure_unix_command(c.as_std_mut());
                c
            }
            ActionKind::ExecutePowerShell { script, no_profile } => {
                let mut c = tokio::process::Command::new("powershell.exe");
                if *no_profile {
                    c.arg("-NoProfile");
                }
                c.args([
                    "-NonInteractive",
                    "-ExecutionPolicy",
                    "Bypass",
                    "-Command",
                    script,
                ]);
                #[cfg(windows)]
                easyjob_platform::windows::configure_windows_command(c.as_std_mut());
                #[cfg(unix)]
                easyjob_platform::unix::configure_unix_command(c.as_std_mut());
                c
            }
        };

        if let Some(wd) = working_dir {
            cmd.current_dir(wd);
        }
        for (k, v) in env {
            cmd.env(k, v);
        }

        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());

        let mut child = cmd.spawn().map_err(|e| Error::Process(e.to_string()))?;
        let pid = child.id();

        let stdout_pipe = child.stdout.take();
        let stderr_pipe = child.stderr.take();

        let stdout_handle = tokio::spawn(read_stream_capped(stdout_pipe));
        let stderr_handle = tokio::spawn(read_stream_capped(stderr_pipe));

        let wait_fut = async { child.wait().await };
        let timeout_duration = Duration::from_secs(timeout_secs.unwrap_or(86400));

        tokio::select! {
            _ = cancel.cancelled() => {
                if let Some(p) = pid {
                    let _ = kill_process_tree(p).await;
                }
                let _ = child.wait().await;
                let stdout = stdout_handle.await.unwrap_or_default();
                let stderr = stderr_handle.await.unwrap_or_default();
                Ok(RunResult {
                    status: ExecutionStatus::Cancelled,
                    exit_code: None,
                    stdout,
                    stderr,
                    error_message: Some("Cancelled by user".to_string()),
                })
            }
            timed_out = tokio::time::timeout(timeout_duration, wait_fut) => {
                match timed_out {
                    Err(_) => {
                        if let Some(p) = pid {
                            let _ = kill_process_tree(p).await;
                        }
                        let _ = child.wait().await;
                        let stdout = stdout_handle.await.unwrap_or_default();
                        let stderr = stderr_handle.await.unwrap_or_default();
                        Ok(RunResult {
                            status: ExecutionStatus::TimedOut,
                            exit_code: None,
                            stdout,
                            stderr,
                            error_message: Some(format!("Timed out after {} seconds", timeout_duration.as_secs())),
                        })
                    }
                    Ok(exit_status) => {
                        let status_code = exit_status.map_err(|e| Error::Process(e.to_string()))?.code();
                        let stdout = stdout_handle.await.unwrap_or_default();
                        let stderr = stderr_handle.await.unwrap_or_default();
                        let success = status_code == Some(0);
                        Ok(RunResult {
                            status: if success { ExecutionStatus::Succeeded } else { ExecutionStatus::Failed },
                            exit_code: status_code,
                            stdout,
                            stderr,
                            error_message: if success { None } else { Some(format!("Exited with code {:?}", status_code)) },
                        })
                    }
                }
            }
        }
    }
}

async fn read_stream_capped<R: tokio::io::AsyncRead + Unpin>(pipe: Option<R>) -> String {
    let mut buf = Vec::new();
    let mut truncated = false;
    if let Some(mut pipe) = pipe {
        let mut chunk = [0u8; 4096];
        while let Ok(n) = pipe.read(&mut chunk).await {
            if n == 0 {
                break;
            }
            if truncated {
                continue;
            }
            if buf.len() + n <= MAX_OUTPUT_BYTES {
                buf.extend_from_slice(&chunk[..n]);
            } else {
                let rem = MAX_OUTPUT_BYTES.saturating_sub(buf.len());
                buf.extend_from_slice(&chunk[..rem]);
                buf.extend_from_slice(TRUNCATION_SUFFIX.as_bytes());
                truncated = true;
            }
        }
    }
    String::from_utf8_lossy(&buf).to_string()
}
