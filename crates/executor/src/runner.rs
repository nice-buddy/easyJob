use easyjob_common::{Error, Result};
use easyjob_domain::action::{Action, ActionKind};
use easyjob_domain::execution::ExecutionStatus;
use easyjob_platform::{kill_process_tree, CommandBuilder};
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::AsyncReadExt;
use tokio_util::sync::CancellationToken;

pub const MAX_OUTPUT_BYTES: usize = 2 * 1024 * 1024; // 2MB
pub const TRUNCATION_SUFFIX: &str = "\n... [output truncated]";
pub const PIPE_DRAIN_TIMEOUT: Duration = Duration::from_millis(1000);

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
            ActionKind::ExecuteCmd { command } => CommandBuilder::new_cmd(command),
            ActionKind::ExecutePowerShell { script, no_profile } => {
                CommandBuilder::new_powershell(script, *no_profile)
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

        let stdout_buf = Arc::new(std::sync::Mutex::new(Vec::new()));
        let stderr_buf = Arc::new(std::sync::Mutex::new(Vec::new()));

        let stdout_handle = tokio::spawn(read_stream_capped(stdout_pipe, stdout_buf.clone()));
        let stderr_handle = tokio::spawn(read_stream_capped(stderr_pipe, stderr_buf.clone()));

        let wait_fut = async { child.wait().await };
        let timeout_duration = Duration::from_secs(timeout_secs.unwrap_or(86400));

        tokio::select! {
            _ = cancel.cancelled() => {
                if let Some(p) = pid {
                    let _ = kill_process_tree(p).await;
                }
                let _ = child.wait().await;
                let (stdout, stderr) = tokio::join!(
                    collect_pipe_output(stdout_handle, stdout_buf),
                    collect_pipe_output(stderr_handle, stderr_buf),
                );
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
                        let (stdout, stderr) = tokio::join!(
                            collect_pipe_output(stdout_handle, stdout_buf),
                            collect_pipe_output(stderr_handle, stderr_buf),
                        );
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
                        let (stdout, stderr) = tokio::join!(
                            collect_pipe_output(stdout_handle, stdout_buf),
                            collect_pipe_output(stderr_handle, stderr_buf),
                        );
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

async fn collect_pipe_output(
    mut handle: tokio::task::JoinHandle<String>,
    buffer: Arc<std::sync::Mutex<Vec<u8>>>,
) -> String {
    match tokio::time::timeout(PIPE_DRAIN_TIMEOUT, &mut handle).await {
        Ok(join_res) => match join_res {
            Ok(s) => s,
            Err(_) => {
                let bytes = buffer.lock().map(|b| b.clone()).unwrap_or_default();
                String::from_utf8_lossy(&bytes).to_string()
            }
        },
        Err(_) => {
            // Timed out waiting for pipe EOF (e.g. grandchild inherited FD)
            handle.abort();
            let bytes = buffer.lock().map(|b| b.clone()).unwrap_or_default();
            String::from_utf8_lossy(&bytes).to_string()
        }
    }
}

async fn read_stream_capped<R: tokio::io::AsyncRead + Unpin>(
    pipe: Option<R>,
    output_buf: Arc<std::sync::Mutex<Vec<u8>>>,
) -> String {
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
            if let Ok(mut buf) = output_buf.lock() {
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
    }
    let buf = output_buf.lock().map(|b| b.clone()).unwrap_or_default();
    String::from_utf8_lossy(&buf).to_string()
}
