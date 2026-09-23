use easyjob_ipc::client::IpcClient;
use easyjob_ipc::transport::system_ipc_path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tokio::sync::Mutex;
use tracing::{info, warn};

#[derive(Clone)]
pub struct AgentManager {
    ipc_path: PathBuf,
    client: Arc<Mutex<Option<IpcClient>>>,
    connect_lock: Arc<Mutex<()>>,
    last_spawn_attempt: Arc<Mutex<Option<std::time::Instant>>>,
    allow_spawn: Arc<AtomicBool>,
    spawned_by_us: Arc<AtomicBool>,
}

impl Default for AgentManager {
    fn default() -> Self {
        Self::new()
    }
}

impl AgentManager {
    pub fn new() -> Self {
        Self::with_ipc_path(system_ipc_path())
    }

    pub fn with_ipc_path(path: PathBuf) -> Self {
        Self {
            ipc_path: path,
            client: Arc::new(Mutex::new(None)),
            connect_lock: Arc::new(Mutex::new(())),
            last_spawn_attempt: Arc::new(Mutex::new(None)),
            allow_spawn: Arc::new(AtomicBool::new(true)),
            spawned_by_us: Arc::new(AtomicBool::new(false)),
        }
    }

    /// 开机自启模式下设为 false：只连接系统服务，绝不拉起新 Agent。
    pub fn set_allow_spawn(&self, allow: bool) {
        self.allow_spawn.store(allow, Ordering::SeqCst);
    }

    pub fn allow_spawn(&self) -> bool {
        self.allow_spawn.load(Ordering::SeqCst)
    }

    /// 本会话是否由桌面端拉起 Agent（退出时据此决定是否 shutdown）。
    pub fn spawned_by_us(&self) -> bool {
        self.spawned_by_us.load(Ordering::SeqCst)
    }

    pub fn client_handle(&self) -> Arc<Mutex<Option<IpcClient>>> {
        self.client.clone()
    }

    pub async fn disconnect(&self) {
        let mut guard = self.client.lock().await;
        *guard = None;
    }

    pub async fn ensure_connected(&self) -> Result<IpcClient, String> {
        // Fast-path: already connected
        {
            let guard = self.client.lock().await;
            if let Some(ref client) = *guard {
                return Ok(client.clone());
            }
        }

        // Acquire connection lock to serialize reconnection attempts
        let _conn_guard = self.connect_lock.lock().await;

        // Double-check after acquiring connect lock
        {
            let guard = self.client.lock().await;
            if let Some(ref client) = *guard {
                return Ok(client.clone());
            }
        }

        if let Ok(client) = IpcClient::connect(&self.ipc_path).await {
            let mut guard = self.client.lock().await;
            *guard = Some(client.clone());
            return Ok(client);
        }

        // Check spawn cooldown (5s) to avoid spawning repeatedly if daemon fails to launch
        let should_spawn = {
            let mut last = self.last_spawn_attempt.lock().await;
            match *last {
                Some(t) if t.elapsed() < Duration::from_secs(5) => false,
                _ => {
                    *last = Some(std::time::Instant::now());
                    true
                }
            }
        };

        if should_spawn {
            if !self.allow_spawn() {
                return Err(
                    "系统服务未运行，请检查开机自启服务状态（桌面端不会另起 Agent）"
                        .to_string(),
                );
            }
            info!("easyjob-agent is not running; attempting to spawn daemon");
            match Self::spawn_agent_process() {
                Ok(()) => self.spawned_by_us.store(true, Ordering::SeqCst),
                Err(e) => warn!("Failed to spawn easyjob-agent: {}", e),
            }
        }

        // Retry connecting without holding the client lock
        for _ in 0..15 {
            tokio::time::sleep(Duration::from_millis(200)).await;
            if let Ok(client) = IpcClient::connect(&self.ipc_path).await {
                info!("Successfully connected to easyjob-agent IPC");
                let mut guard = self.client.lock().await;
                *guard = Some(client.clone());
                return Ok(client);
            }
        }

        Err("Failed to connect to easyjob-agent daemon".to_string())
    }

    pub async fn call(
        &self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        let client = self.ensure_connected().await?;
        match client.call(method, params.clone()).await {
            Ok(res) => Ok(res),
            Err(e) if is_connection_error(&e) => {
                warn!(
                    "IPC call '{}' failed with connection error ({}). Reconnecting and retrying...",
                    method, e
                );
                self.disconnect().await;
                let client = self.ensure_connected().await?;
                client.call(method, params).await.map_err(|e| e.to_string())
            }
            Err(e) => Err(e.to_string()),
        }
    }

    pub async fn shutdown_agent(&self) {
        let client_opt = {
            let guard = self.client.lock().await;
            guard.clone()
        };
        if let Some(client) = client_opt {
            let _ = client.call("agent.shutdown", serde_json::json!({})).await;
            self.disconnect().await;
        } else if let Ok(client) = IpcClient::connect(&self.ipc_path).await {
            let _ = client.call("agent.shutdown", serde_json::json!({})).await;
        }
    }

    pub async fn restart_agent(&self) -> Result<bool, String> {
        if !self.allow_spawn() {
            return Err("开机自启模式下请通过系统服务管理器重启 Agent".to_string());
        }
        info!("Restarting easyjob-agent daemon requested");
        if let Ok(client) = self.ensure_connected().await {
            let _ = client.call("agent.shutdown", serde_json::json!({})).await;
        }
        self.disconnect().await;
        tokio::time::sleep(Duration::from_millis(500)).await;
        Self::spawn_agent_process()?;
        self.ensure_connected().await?;
        Ok(true)
    }

    fn find_agent_binary() -> Result<PathBuf, String> {
        #[cfg(target_os = "windows")]
        let bin_name = "easyjob-agent.exe";
        #[cfg(not(target_os = "windows"))]
        let bin_name = "easyjob-agent";

        if let Ok(current_exe) = std::env::current_exe() {
            let mut dir = current_exe;
            dir.pop();

            // 1. Next to current executable (e.g. MacOS/easyjob-agent or Program Files\easyJob\easyjob-agent.exe)
            let target_bin = dir.join(bin_name);
            if target_bin.exists() {
                return Ok(target_bin);
            }

            // 2. In target/debug or target/release if run from target/debug/deps
            if dir.ends_with("deps") {
                dir.pop();
                let target_bin = dir.join(bin_name);
                if target_bin.exists() {
                    return Ok(target_bin);
                }
            }

            // 3. In Resources directory (macOS app bundle: Contents/Resources/easyjob-agent)
            let resources_bin = dir.join("../Resources").join(bin_name);
            if resources_bin.exists() {
                return Ok(resources_bin);
            }

            // 4. In src-tauri/binaries (development fallback)
            let binaries_bin = dir.join("binaries").join(bin_name);
            if binaries_bin.exists() {
                return Ok(binaries_bin);
            }
        }

        // 5. Search relative to current working directory (e.g. target/release or target/debug)
        if let Ok(cwd) = std::env::current_dir() {
            for sub in &["target/release", "target/debug"] {
                let candidate = cwd.join(sub).join(bin_name);
                if candidate.exists() {
                    return Ok(candidate);
                }
            }
        }

        // 6. Search PATH fallback
        Ok(PathBuf::from(bin_name))
    }

    fn spawn_agent_process() -> Result<(), String> {
        let bin = Self::find_agent_binary()?;
        info!("Spawning easyjob-agent daemon from: {:?}", bin);
        let mut cmd = std::process::Command::new(bin);
        cmd.arg("--daemon");
        let exe_dir = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.to_path_buf()));
        let data_dir = easyjob_platform::startup::system_data_dir(exe_dir.as_deref());
        cmd.arg("--data-dir");
        cmd.arg(&data_dir);
        cmd.stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());

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

fn is_connection_error(err: &easyjob_common::Error) -> bool {
    match err {
        easyjob_common::Error::Io(_) => true,
        easyjob_common::Error::Other(msg) => {
            let lower = msg.to_lowercase();
            lower.contains("failed to send ipc request")
                || lower.contains("channel closed")
                || lower.contains("connection")
                || lower.contains("broken pipe")
                || lower.contains("reset by peer")
                || lower.contains("timed out")
        }
        _ => false,
    }
}
