use easyjob_common::{Error, Result};
use easyjob_ipc::client::IpcClient;
use fs2::FileExt;
use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tracing::{info, warn};

#[derive(Debug)]
pub enum LockOutcome {
    Acquired(SingleInstanceLock),
    AlreadyRunning,
}

#[derive(Debug)]
pub struct SingleInstanceLock {
    _file: File,
    path: PathBuf,
}

impl Drop for SingleInstanceLock {
    fn drop(&mut self) {
        let _ = self._file.unlock();
    }
}

impl SingleInstanceLock {
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub async fn acquire(lock_path: &Path, ipc_path: &Path) -> Result<LockOutcome> {
        if let Some(parent) = lock_path.parent() {
            std::fs::create_dir_all(parent).map_err(Error::Io)?;
        }

        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(lock_path)
            .map_err(Error::Io)?;

        match file.try_lock_exclusive() {
            Ok(()) => {
                info!("Acquired single-instance lock at {:?}", lock_path);
                Ok(LockOutcome::Acquired(Self {
                    _file: file,
                    path: lock_path.to_path_buf(),
                }))
            }
            Err(e) if is_lock_contended(&e) => {
                // Lock held by another process: probe IPC endpoint
                if probe_agent_status(ipc_path).await {
                    return Ok(LockOutcome::AlreadyRunning);
                }

                // Probe failed -> Stale lock recovery
                warn!("Stale lock detected at {:?}; recovering", lock_path);
                drop(file);
                let _ = std::fs::remove_file(lock_path);

                let recovered_file = OpenOptions::new()
                    .read(true)
                    .write(true)
                    .create(true)
                    .truncate(false)
                    .open(lock_path)
                    .map_err(Error::Io)?;

                recovered_file.try_lock_exclusive().map_err(|err| {
                    Error::Other(format!("Failed to acquire recovered lock: {}", err))
                })?;

                info!(
                    "Successfully recovered and acquired single-instance lock at {:?}",
                    lock_path
                );
                Ok(LockOutcome::Acquired(Self {
                    _file: recovered_file,
                    path: lock_path.to_path_buf(),
                }))
            }
            Err(e) => Err(Error::Io(e)),
        }
    }
}

async fn probe_agent_status(ipc_path: &Path) -> bool {
    let connect_timeout = Duration::from_millis(500);
    let rpc_timeout = Duration::from_millis(500);

    let client = match tokio::time::timeout(connect_timeout, IpcClient::connect(ipc_path)).await {
        Ok(Ok(client)) => client,
        _ => return false,
    };

    client
        .call_timeout("agent.status", serde_json::json!({}), rpc_timeout)
        .await
        .is_ok()
}

fn is_lock_contended(err: &std::io::Error) -> bool {
    err.kind() == std::io::ErrorKind::WouldBlock || err.raw_os_error() == Some(33)
}
