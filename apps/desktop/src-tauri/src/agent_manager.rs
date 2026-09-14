use easyjob_ipc::client::IpcClient;
use easyjob_ipc::default_ipc_path;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tracing::{info, warn};

#[derive(Clone)]
pub struct AgentManager {
    ipc_path: PathBuf,
    client: Arc<Mutex<Option<IpcClient>>>,
}

impl Default for AgentManager {
    fn default() -> Self {
        Self::new()
    }
}

impl AgentManager {
    pub fn new() -> Self {
        Self::with_ipc_path(default_ipc_path())
    }

    pub fn with_ipc_path(path: PathBuf) -> Self {
        Self {
            ipc_path: path,
            client: Arc::new(Mutex::new(None)),
        }
    }

    pub fn client_handle(&self) -> Arc<Mutex<Option<IpcClient>>> {
        self.client.clone()
    }

    pub async fn disconnect(&self) {
        let mut guard = self.client.lock().await;
        *guard = None;
    }

    pub async fn ensure_connected(&self) -> Result<IpcClient, String> {
        let mut guard = self.client.lock().await;
        if let Some(ref client) = *guard {
            return Ok(client.clone());
        }

        if let Ok(client) = IpcClient::connect(&self.ipc_path).await {
            *guard = Some(client.clone());
            return Ok(client);
        }

        // Try spawning agent binary
        info!("easyjob-agent is not running; attempting to spawn daemon");
        if let Err(e) = Self::spawn_agent_process() {
            warn!("Failed to spawn easyjob-agent: {}", e);
        }

        // Retry connecting with backoff
        for _ in 0..15 {
            tokio::time::sleep(Duration::from_millis(200)).await;
            if let Ok(client) = IpcClient::connect(&self.ipc_path).await {
                info!("Successfully connected to easyjob-agent IPC");
                *guard = Some(client.clone());
                return Ok(client);
            }
        }

        Err("Failed to connect to easyjob-agent daemon".to_string())
    }

    fn find_agent_binary() -> Result<PathBuf, String> {
        if let Ok(current_exe) = std::env::current_exe() {
            let mut dir = current_exe;
            dir.pop();
            #[cfg(target_os = "windows")]
            let bin_name = "easyjob-agent.exe";
            #[cfg(not(target_os = "windows"))]
            let bin_name = "easyjob-agent";

            let target_bin = dir.join(bin_name);
            if target_bin.exists() {
                return Ok(target_bin);
            }

            if dir.ends_with("deps") {
                dir.pop();
            }
            let target_bin = dir.join(bin_name);
            if target_bin.exists() {
                return Ok(target_bin);
            }
        }

        // Search PATH fallback
        Ok(PathBuf::from("easyjob-agent"))
    }

    fn spawn_agent_process() -> Result<(), String> {
        let bin = Self::find_agent_binary()?;
        let mut cmd = std::process::Command::new(bin);
        cmd.arg("--daemon");

        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x08000000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }

        cmd.spawn()
            .map_err(|e| format!("Spawn process error: {}", e))?;
        Ok(())
    }
}
