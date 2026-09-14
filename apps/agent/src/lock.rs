use easyjob_common::{Error, Result};
use fs2::FileExt;
use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};
use tracing::info;

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

    pub async fn acquire(lock_path: &Path, _ipc_path: &Path) -> Result<LockOutcome> {
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
                info!(
                    "Another easyjob-agent instance is already running (lock contended at {:?})",
                    lock_path
                );
                Ok(LockOutcome::AlreadyRunning)
            }
            Err(e) => Err(Error::Io(e)),
        }
    }
}

fn is_lock_contended(err: &std::io::Error) -> bool {
    err.kind() == std::io::ErrorKind::WouldBlock || err.raw_os_error() == Some(33)
}
