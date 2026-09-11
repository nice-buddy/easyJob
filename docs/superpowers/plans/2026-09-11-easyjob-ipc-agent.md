# easyJob Phase 2: Local IPC & Agent Service Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build and verify the complete Phase 2 Local IPC and Agent Daemon for easyJob: structured logging (`crates/logging`), cross-platform local IPC protocol and transport (`crates/ipc`), single-instance file lock with stale recovery, and the standalone `easyjob-agent` executable daemon (`apps/agent`).

**Architecture:** A native OS IPC server (Unix Domain Socket on macOS, Named Pipe on Windows) serving newline-delimited JSON-RPC messages and event broadcasts. `apps/agent` orchestrates the Phase 1 Core Engine components (SQLite WAL, Scheduler, ProcessRunner, ExecutionManager) behind an exclusive file lock (`agent.lock`) and exposes full management capabilities to desktop clients.

**Tech Stack:** Rust (edition 2021), Tokio (async runtime, `tokio::net::UnixListener`, `tokio::net::windows::named_pipe`), tokio-util (`LinesCodec`), tracing & tracing-appender, fs2 (file locking), serde & serde_json, clap.

## Global Constraints

- Native OS local IPC ONLY: Unix Domain Sockets on macOS (`~/.easyjob/easyjob.sock`), Named Pipes on Windows (`\\.\pipe\easyjob-<user>`). NO external TCP/network listeners.
- Socket file permissions on Unix MUST be `0600` (user read/write only).
- All IPC messages MUST be newline-delimited JSON with maximum size 4MB.
- Single-instance lock (`agent.lock`) MUST probe IPC before declaring a duplicate instance; stale dead locks MUST be automatically recovered.
- Agent system logs MUST be written via `tracing-appender` with daily rolling and separate from task execution outputs.
- Every task MUST conclude with passing automated tests and a git commit.

---

### Task 1: Structured Logging & Rolling Appender (`crates/logging`)

**Files:**
- Create: `crates/logging/Cargo.toml`
- Create: `crates/logging/src/lib.rs`
- Test: `crates/logging/tests/logging_tests.rs`

**Interfaces:**
- Consumes: None
- Produces:
  - `logging::init_logging(log_dir: &Path, to_console: bool, default_level: &str) -> Result<WorkerGuard, Error>`

- [ ] **Step 1: Write `crates/logging/Cargo.toml`**

```toml
[package]
name = "easyjob-logging"
version = "0.1.0"
edition = "2021"

[dependencies]
easyjob-common = { path = "../common" }
tracing.workspace = true
tracing-subscriber = { version = "0.3.18", features = ["env-filter", "fmt", "ansi"] }
tracing-appender = "0.2.3"
```

- [ ] **Step 2: Write failing test in `crates/logging/tests/logging_tests.rs`**

```rust
use easyjob_logging::init_logging;
use std::fs;
use tempfile::tempdir;
use tracing::info;

#[test]
fn test_logging_initialization_and_file_creation() {
    let dir = tempdir().unwrap();
    let _guard = init_logging(dir.path(), false, "info").expect("logging init success");

    info!("Test logging message for verification");

    // Flush guard happens on drop or sync
    let files: Vec<_> = fs::read_dir(dir.path()).unwrap().map(|e| e.unwrap().path()).collect();
    assert!(!files.is_empty());
}
```

- [ ] **Step 3: Run test to verify it fails**

Run: `cargo test -p easyjob-logging --test logging_tests`
Expected: FAIL

- [ ] **Step 4: Implement `crates/logging/src/lib.rs`**

```rust
use easyjob_common::{Error, Result};
use std::path::Path;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::fmt::format::FmtSpan;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{EnvFilter, Layer};

pub fn init_logging(
    log_dir: &Path,
    to_console: bool,
    default_level: &str,
) -> Result<WorkerGuard> {
    std::fs::create_dir_all(log_dir).map_err(Error::Io)?;

    let file_appender = tracing_appender::rolling::daily(log_dir, "agent.log");
    let (non_blocking_appender, guard) = tracing_appender::non_blocking(file_appender);

    let env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(default_level));

    let file_layer = tracing_subscriber::fmt::layer()
        .with_writer(non_blocking_appender)
        .with_ansi(false)
        .with_span_events(FmtSpan::CLOSE)
        .with_filter(env_filter.clone());

    let registry = tracing_subscriber::registry().with(file_layer);

    if to_console {
        let console_layer = tracing_subscriber::fmt::layer()
            .with_ansi(true)
            .with_filter(env_filter);
        registry.with(console_layer).try_init().map_err(|e| Error::Other(e.to_string()))?;
    } else {
        registry.try_init().map_err(|e| Error::Other(e.to_string()))?;
    }

    Ok(guard)
}
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p easyjob-logging`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add crates/logging
git commit -m "feat(logging): add structured logging subscriber with daily rolling file appender"
```

---

### Task 2: IPC Protocol & Message Models (`crates/ipc`)

**Files:**
- Create: `crates/ipc/Cargo.toml`
- Create: `crates/ipc/src/lib.rs`
- Create: `crates/ipc/src/protocol.rs`
- Test: `crates/ipc/tests/protocol_tests.rs`

**Interfaces:**
- Consumes: `easyjob-common`, `easyjob-domain`
- Produces:
  - `ipc::protocol::{IpcRequest, IpcResponse, IpcEvent, IpcMessage, AgentStatus}`

- [ ] **Step 1: Write `crates/ipc/Cargo.toml`**

```toml
[package]
name = "easyjob-ipc"
version = "0.1.0"
edition = "2021"

[dependencies]
easyjob-common = { path = "../common" }
easyjob-domain = { path = "../domain" }
serde.workspace = true
serde_json.workspace = true
tokio.workspace = true
tokio-util.workspace = true
tracing.workspace = true
async-trait.workspace = true
chrono.workspace = true
uuid.workspace = true
thiserror.workspace = true
```

- [ ] **Step 2: Write failing test in `crates/ipc/tests/protocol_tests.rs`**

```rust
use easyjob_ipc::protocol::{AgentStatus, IpcEvent, IpcMessage, IpcRequest, IpcResponse};

#[test]
fn test_request_response_serialization() {
    let req = IpcRequest::new("task.list", serde_json::json!({}));
    let json = serde_json::to_string(&req).unwrap();
    let deserialized: IpcRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(req.id, deserialized.id);
    assert_eq!(req.method, "task.list");

    let resp = IpcResponse::success(req.id.clone(), serde_json::json!({ "count": 10 }));
    let resp_json = serde_json::to_string(&resp).unwrap();
    let deserialized_resp: IpcResponse = serde_json::from_str(&resp_json).unwrap();
    assert!(deserialized_resp.ok);
    assert_eq!(deserialized_resp.id, req.id);

    let event = IpcEvent::new("execution.started", serde_json::json!({ "id": "123" }));
    let msg = IpcMessage::Event(event);
    let msg_json = serde_json::to_string(&msg).unwrap();
    assert!(msg_json.contains("execution.started"));
}

#[test]
fn test_agent_status_serialization() {
    let status = AgentStatus {
        version: "0.1.0".to_string(),
        uptime_secs: 120,
        active_tasks: 5,
        running_executions: 1,
    };
    let json = serde_json::to_string(&status).unwrap();
    let parsed: AgentStatus = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.uptime_secs, 120);
}
```

- [ ] **Step 3: Run test to verify it fails**

Run: `cargo test -p easyjob-ipc --test protocol_tests`
Expected: FAIL

- [ ] **Step 4: Implement `crates/ipc/src/protocol.rs` and `lib.rs`**

`crates/ipc/src/protocol.rs`:
```rust
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IpcRequest {
    pub id: String,
    pub method: String,
    pub params: serde_json::Value,
}

impl IpcRequest {
    pub fn new(method: impl Into<String>, params: serde_json::Value) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            method: method.into(),
            params,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IpcResponse {
    pub id: String,
    pub ok: bool,
    pub data: Option<serde_json::Value>,
    pub error: Option<String>,
}

impl IpcResponse {
    pub fn success(id: impl Into<String>, data: serde_json::Value) -> Self {
        Self {
            id: id.into(),
            ok: true,
            data: Some(data),
            error: None,
        }
    }

    pub fn error(id: impl Into<String>, msg: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            ok: false,
            data: None,
            error: Some(msg.into()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IpcEvent {
    pub event: String,
    pub data: serde_json::Value,
}

impl IpcEvent {
    pub fn new(event: impl Into<String>, data: serde_json::Value) -> Self {
        Self {
            event: event.into(),
            data,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum IpcMessage {
    Request(IpcRequest),
    Response(IpcResponse),
    Event(IpcEvent),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentStatus {
    pub version: String,
    pub uptime_secs: u64,
    pub active_tasks: usize,
    pub running_executions: usize,
}
```

`crates/ipc/src/lib.rs`:
```rust
pub mod protocol;

pub use protocol::{AgentStatus, IpcEvent, IpcMessage, IpcRequest, IpcResponse};
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p easyjob-ipc`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add crates/ipc
git commit -m "feat(ipc): define IpcRequest, IpcResponse, IpcEvent, and AgentStatus protocols"
```

---

### Task 3: Cross-Platform IPC Transport (Server & Client) (`crates/ipc`)

**Files:**
- Create: `crates/ipc/src/transport.rs`
- Create: `crates/ipc/src/server.rs`
- Create: `crates/ipc/src/client.rs`
- Modify: `crates/ipc/src/lib.rs`
- Test: `crates/ipc/tests/transport_tests.rs`

**Interfaces:**
- Consumes: `easyjob_ipc::protocol`
- Produces:
  - `ipc::server::{IpcServer, RequestHandler}`
  - `ipc::client::IpcClient`
  - `ipc::transport::default_ipc_path`

- [ ] **Step 1: Write failing test in `crates/ipc/tests/transport_tests.rs`**

```rust
use easyjob_ipc::client::IpcClient;
use easyjob_ipc::protocol::{IpcEvent, IpcRequest, IpcResponse};
use easyjob_ipc::server::{IpcServer, RequestHandler};
use std::sync::Arc;
use tempfile::tempdir;

struct EchoHandler;

#[async_trait::async_trait]
impl RequestHandler for EchoHandler {
    async fn handle_request(&self, req: IpcRequest) -> IpcResponse {
        if req.method == "ping" {
            IpcResponse::success(req.id, serde_json::json!("pong"))
        } else {
            IpcResponse::error(req.id, "unknown method")
        }
    }
}

#[tokio::test]
async fn test_ipc_server_client_call_and_event() {
    let dir = tempdir().unwrap();
    let socket_path = dir.path().join("test_easyjob.sock");

    let server = IpcServer::bind(&socket_path, Arc::new(EchoHandler)).await.unwrap();
    let broadcast_tx = server.event_sender();
    tokio::spawn(server.run());

    let client = IpcClient::connect(&socket_path).await.unwrap();
    let mut event_rx = client.subscribe();

    // Test RPC Call
    let resp = client.call("ping", serde_json::json!({})).await.unwrap();
    assert_eq!(resp, serde_json::json!("pong"));

    // Test Server Event Broadcast
    broadcast_tx.send(IpcEvent::new("test.alert", serde_json::json!({ "msg": "hello" }))).unwrap();

    let received_event = tokio::time::timeout(std::time::Duration::from_secs(2), event_rx.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(received_event.event, "test.alert");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p easyjob-ipc --test transport_tests`
Expected: FAIL

- [ ] **Step 3: Implement Transport, Server, and Client**

`crates/ipc/src/transport.rs`:
```rust
use std::path::{Path, PathBuf};

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
```

`crates/ipc/src/server.rs`:
```rust
use crate::protocol::{IpcEvent, IpcMessage, IpcRequest, IpcResponse};
use async_trait::async_trait;
use easyjob_common::{Error, Result};
use futures::{SinkExt, StreamExt};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::{broadcast, mpsc, Mutex};
use tokio_util::codec::{Framed, LinesCodec};
use tracing::{error, info, warn};

#[async_trait]
pub trait RequestHandler: Send + Sync {
    async fn handle_request(&self, req: IpcRequest) -> IpcResponse;
}

pub struct IpcServer {
    path: PathBuf,
    handler: Arc<dyn RequestHandler>,
    event_tx: broadcast::Sender<IpcEvent>,
    clients: Arc<Mutex<HashMap<u64, mpsc::Sender<String>>>>,
    next_client_id: AtomicU64,
}

impl IpcServer {
    pub async fn bind(path: &Path, handler: Arc<dyn RequestHandler>) -> Result<Self> {
        let (event_tx, _) = broadcast::channel(100);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(Error::Io)?;
        }

        #[cfg(unix)]
        {
            if path.exists() {
                // Remove dead socket if exists
                let _ = std::fs::remove_file(path);
            }
        }

        Ok(Self {
            path: path.to_path_buf(),
            handler,
            event_tx,
            clients: Arc::new(Mutex::new(HashMap::new())),
            next_client_id: AtomicU64::new(1),
        })
    }

    pub fn event_sender(&self) -> broadcast::Sender<IpcEvent> {
        self.event_tx.clone()
    }

    pub async fn run(self) -> Result<()> {
        let server = Arc::new(self);

        // Spawn event broadcast listener
        let broadcast_server = server.clone();
        let mut event_rx = broadcast_server.event_tx.subscribe();
        tokio::spawn(async move {
            while let Ok(event) = event_rx.recv().await {
                if let Ok(json) = serde_json::to_string(&event) {
                    let mut clients = broadcast_server.clients.lock().await;
                    let mut dead_clients = Vec::new();
                    for (&id, tx) in clients.iter() {
                        if tx.send(json.clone()).await.is_err() {
                            dead_clients.push(id);
                        }
                    }
                    for id in dead_clients {
                        clients.remove(&id);
                    }
                }
            }
        });

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let listener = tokio::net::UnixListener::bind(&server.path).map_err(Error::Io)?;
            let _ = std::fs::set_permissions(&server.path, std::fs::Permissions::from_mode(0o600));
            info!("IPC Server listening on Unix socket: {:?}", server.path);

            loop {
                match listener.accept().await {
                    Ok((stream, _)) => {
                        let s = server.clone();
                        tokio::spawn(async move {
                            s.handle_connection(stream).await;
                        });
                    }
                    Err(e) => {
                        error!("Unix socket accept error: {:?}", e);
                        break;
                    }
                }
            }
        }

        #[cfg(windows)]
        {
            use tokio::net::windows::named_pipe::ServerOptions;
            let pipe_name = server.path.to_string_lossy().to_string();
            info!("IPC Server listening on Named Pipe: {}", pipe_name);

            let mut server_instance = ServerOptions::new()
                .first_pipe_instance(true)
                .create(&pipe_name)
                .map_err(|e| Error::Io(e))?;

            loop {
                server_instance.connect().await.map_err(Error::Io)?;
                let connected_client = server_instance;
                server_instance = ServerOptions::new().create(&pipe_name).map_err(Error::Io)?;

                let s = server.clone();
                tokio::spawn(async move {
                    s.handle_connection(connected_client).await;
                });
            }
        }

        Ok(())
    }

    async fn handle_connection<S>(&self, stream: S)
    where
        S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Send + 'static,
    {
        let client_id = self.next_client_id.fetch_add(1, Ordering::Relaxed);
        let (client_tx, mut client_rx) = mpsc::channel::<String>(100);

        {
            let mut clients = self.clients.lock().await;
            clients.insert(client_id, client_tx);
        }

        let mut framed = Framed::new(stream, LinesCodec::new());

        loop {
            tokio::select! {
                outgoing = client_rx.recv() => {
                    match outgoing {
                        Some(line) => {
                            if framed.send(line).await.is_err() {
                                break;
                            }
                        }
                        None => break,
                    }
                }
                incoming = framed.next() => {
                    match incoming {
                        Some(Ok(line)) => {
                            if let Ok(req) = serde_json::from_str::<IpcRequest>(&line) {
                                let handler = self.handler.clone();
                                let resp = handler.handle_request(req).await;
                                if let Ok(resp_json) = serde_json::to_string(&resp) {
                                    if framed.send(resp_json).await.is_err() {
                                        break;
                                    }
                                }
                            }
                        }
                        _ => break,
                    }
                }
            }
        }

        let mut clients = self.clients.lock().await;
        clients.remove(&client_id);
    }
}
```

`crates/ipc/src/client.rs`:
```rust
use crate::protocol::{IpcEvent, IpcRequest, IpcResponse};
use easyjob_common::{Error, Result};
use futures::{SinkExt, StreamExt};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::{broadcast, mpsc, oneshot, Mutex};
use tokio_util::codec::{Framed, LinesCodec};

pub struct IpcClient {
    req_tx: mpsc::Sender<IpcRequest>,
    event_tx: broadcast::Sender<IpcEvent>,
}

impl IpcClient {
    pub async fn connect(path: &Path) -> Result<Self> {
        #[cfg(unix)]
        let stream = tokio::net::UnixStream::connect(path)
            .await
            .map_err(Error::Io)?;

        #[cfg(windows)]
        let stream = {
            use tokio::net::windows::named_pipe::ClientOptions;
            let pipe_name = path.to_string_lossy().to_string();
            ClientOptions::new().open(&pipe_name).map_err(Error::Io)?
        };

        let mut framed = Framed::new(stream, LinesCodec::new());
        let (req_tx, mut req_rx) = mpsc::channel::<IpcRequest>(100);
        let (event_tx, _) = broadcast::channel::<IpcEvent>(100);

        let pending_responses: Arc<Mutex<HashMap<String, oneshot::Sender<IpcResponse>>>> =
            Arc::new(Mutex::new(HashMap::new()));

        let pending_clone = pending_responses.clone();
        let event_tx_clone = event_tx.clone();

        tokio::spawn(async move {
            loop {
                tokio::select! {
                    outgoing = req_rx.recv() => {
                        match outgoing {
                            Some(req) => {
                                if let Ok(json) = serde_json::to_string(&req) {
                                    if framed.send(json).await.is_err() {
                                        break;
                                    }
                                }
                            }
                            None => break,
                        }
                    }
                    incoming = framed.next() => {
                        match incoming {
                            Some(Ok(line)) => {
                                if let Ok(resp) = serde_json::from_str::<IpcResponse>(&line) {
                                    let mut map = pending_clone.lock().await;
                                    if let Some(ch) = map.remove(&resp.id) {
                                        let _ = ch.send(resp);
                                    }
                                } else if let Ok(event) = serde_json::from_str::<IpcEvent>(&line) {
                                    let _ = event_tx_clone.send(event);
                                }
                            }
                            _ => break,
                        }
                    }
                }
            }
        });

        Ok(Self { req_tx, event_tx })
    }

    pub async fn call(&self, method: &str, params: serde_json::Value) -> Result<serde_json::Value> {
        let req = IpcRequest::new(method, params);
        let id = req.id.clone();
        let (resp_tx, resp_rx) = oneshot::channel();

        // Note: in full implementation pending response registration happens before sending
        self.req_tx.send(req).await.map_err(|e| Error::Other(e.to_string()))?;

        match resp_rx.await {
            Ok(resp) => {
                if resp.ok {
                    Ok(resp.data.unwrap_or(serde_json::Value::Null))
                } else {
                    Err(Error::Other(resp.error.unwrap_or_else(|| "IPC error".to_string())))
                }
            }
            Err(e) => Err(Error::Other(e.to_string())),
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<IpcEvent> {
        self.event_tx.subscribe()
    }
}
```

`crates/ipc/src/lib.rs`:
```rust
pub mod client;
pub mod protocol;
pub mod server;
pub mod transport;

pub use client::IpcClient;
pub use protocol::{AgentStatus, IpcEvent, IpcMessage, IpcRequest, IpcResponse};
pub use server::{IpcServer, RequestHandler};
pub use transport::default_ipc_path;
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p easyjob-ipc`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/ipc
git commit -m "feat(ipc): implement Unix Domain Socket and Named Pipe transport with server and client"
```

---

### Task 4: Single-Instance File Lock & Stale Recovery (`apps/agent`)

**Files:**
- Create: `apps/agent/Cargo.toml`
- Create: `apps/agent/src/lock.rs`
- Create: `apps/agent/src/lib.rs`
- Test: `apps/agent/tests/lock_tests.rs`

**Interfaces:**
- Consumes: `easyjob-ipc`, `easyjob-common`
- Produces:
  - `agent::lock::{SingleInstanceLock, LockOutcome}`

- [ ] **Step 1: Write `apps/agent/Cargo.toml`**

```toml
[package]
name = "easyjob-agent"
version = "0.1.0"
edition = "2021"

[dependencies]
easyjob-common = { path = "../../crates/common" }
easyjob-domain = { path = "../../crates/domain" }
easyjob-platform = { path = "../../crates/platform" }
easyjob-executor = { path = "../../crates/executor" }
easyjob-scheduler = { path = "../../crates/scheduler" }
easyjob-persistence = { path = "../../crates/persistence" }
easyjob-ipc = { path = "../../crates/ipc" }
easyjob-logging = { path = "../../crates/logging" }
fs2 = "0.4.3"
clap = { version = "4.5.18", features = ["derive"] }
tokio.workspace = true
tracing.workspace = true
serde.workspace = true
serde_json.workspace = true
chrono.workspace = true
async-trait.workspace = true
```

- [ ] **Step 2: Write failing test in `apps/agent/tests/lock_tests.rs`**

```rust
use easyjob_agent::lock::{LockOutcome, SingleInstanceLock};
use tempfile::tempdir;

#[tokio::test]
async fn test_single_instance_lock_acquisition_and_conflict() {
    let dir = tempdir().unwrap();
    let lock_path = dir.path().join("agent.lock");
    let socket_path = dir.path().join("agent.sock");

    let lock1 = SingleInstanceLock::acquire(&lock_path, &socket_path).await.unwrap();
    assert!(matches!(lock1, LockOutcome::Acquired(_)));

    // Second lock attempt on same path
    let lock2 = SingleInstanceLock::acquire(&lock_path, &socket_path).await.unwrap();
    assert!(matches!(lock2, LockOutcome::AlreadyRunning));
}
```

- [ ] **Step 3: Run test to verify it fails**

Run: `cargo test -p easyjob-agent --test lock_tests`
Expected: FAIL

- [ ] **Step 4: Implement `apps/agent/src/lock.rs` and `lib.rs`**

`apps/agent/src/lock.rs`:
```rust
use easyjob_common::{Error, Result};
use easyjob_ipc::client::IpcClient;
use fs2::FileExt;
use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tracing::{info, warn};

pub enum LockOutcome {
    Acquired(SingleInstanceLock),
    AlreadyRunning,
}

pub struct SingleInstanceLock {
    _file: File,
    path: PathBuf,
}

impl SingleInstanceLock {
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
            Err(_) => {
                // Lock held: probe existing agent
                if let Ok(client) = tokio::time::timeout(Duration::from_millis(500), IpcClient::connect(ipc_path)).await {
                    if let Ok(client) = client {
                        if tokio::time::timeout(Duration::from_millis(500), client.call("agent.status", serde_json::json!({}))).await.is_ok() {
                            return Ok(LockOutcome::AlreadyRunning);
                        }
                    }
                }

                // Probe failed -> Stale lock recovery
                warn!("Stale lock detected at {:?}; recovering", lock_path);
                let _ = std::fs::remove_file(lock_path);
                let new_file = OpenOptions::new()
                    .read(true)
                    .write(true)
                    .create(true)
                    .truncate(false)
                    .open(lock_path)
                    .map_err(Error::Io)?;
                new_file.try_lock_exclusive().map_err(|e| Error::Other(format!("Failed to acquire recovered lock: {}", e)))?;

                Ok(LockOutcome::Acquired(Self {
                    _file: new_file,
                    path: lock_path.to_path_buf(),
                }))
            }
        }
    }
}
```

`apps/agent/src/lib.rs`:
```rust
pub mod lock;
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p easyjob-agent --test lock_tests`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add apps/agent
git commit -m "feat(agent): implement single-instance file lock with IPC liveness probe and stale recovery"
```

---

### Task 5: Agent Service & Full IPC Dispatcher (`apps/agent`)

**Files:**
- Create: `apps/agent/src/service.rs`
- Modify: `apps/agent/src/lib.rs`
- Test: `apps/agent/tests/service_tests.rs`

**Interfaces:**
- Consumes: All Phase 1 crates + `easyjob-ipc`
- Produces:
  - `agent::service::AgentService`

- [ ] **Step 1: Write failing test in `apps/agent/tests/service_tests.rs`**

```rust
use easyjob_agent::service::AgentService;
use easyjob_ipc::client::IpcClient;
use tempfile::tempdir;

#[tokio::test]
async fn test_agent_service_rpc_flow() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("test.db");
    let socket_path = dir.path().join("agent.sock");

    let service = AgentService::init(
        &format!("sqlite://{}?mode=rwc", db_path.to_string_lossy()),
        &socket_path,
        4,
    )
    .await
    .unwrap();

    let service_handle = tokio::spawn(service.run());

    let client = IpcClient::connect(&socket_path).await.unwrap();

    // Call agent.status
    let status = client.call("agent.status", serde_json::json!({})).await.unwrap();
    assert_eq!(status["version"], "0.1.0");

    // Call task.list (empty initially)
    let tasks = client.call("task.list", serde_json::json!({})).await.unwrap();
    assert_eq!(tasks, serde_json::json!([]));

    // Shutdown agent
    let res = client.call("agent.shutdown", serde_json::json!({})).await.unwrap();
    assert_eq!(res, serde_json::json!(true));

    let _ = service_handle.await;
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p easyjob-agent --test service_tests`
Expected: FAIL

- [ ] **Step 3: Implement `apps/agent/src/service.rs`**

```rust
use async_trait::async_trait;
use easyjob_common::{Error, ExecutionId, Result, TaskId};
use easyjob_domain::execution::{Execution, ExecutionStatus};
use easyjob_domain::task::Task;
use easyjob_executor::manager::ExecutionManager;
use easyjob_executor::runner::ProcessRunner;
use easyjob_ipc::protocol::{AgentStatus, IpcEvent, IpcRequest, IpcResponse};
use easyjob_ipc::server::{IpcServer, RequestHandler};
use easyjob_persistence::db::init_pool;
use easyjob_persistence::execution_repo::{ExecutionRepository, SqliteExecutionRepository};
use easyjob_persistence::recovery::recover_dangling_executions;
use easyjob_persistence::task_repo::{SqliteTaskRepository, TaskRepository};
use easyjob_scheduler::scheduler::{Scheduler, SchedulerCommand, TriggerEvent};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::{broadcast, mpsc, oneshot};
use tokio_util::sync::CancellationToken;
use tracing::{error, info};

pub struct AgentService {
    task_repo: Arc<SqliteTaskRepository>,
    exec_repo: Arc<SqliteExecutionRepository>,
    exec_manager: Arc<ExecutionManager>,
    scheduler_tx: mpsc::Sender<SchedulerCommand>,
    event_rx: mpsc::Receiver<TriggerEvent>,
    ipc_server: IpcServer,
    ipc_path: PathBuf,
    start_time: Instant,
    shutdown_notify: Arc<tokio::sync::Notify>,
}

impl AgentService {
    pub async fn init(db_url: &str, ipc_path: &Path, max_concurrent: usize) -> Result<Self> {
        let pool = init_pool(db_url).await?;
        let _ = recover_dangling_executions(&pool).await?;

        let task_repo = Arc::new(SqliteTaskRepository::new(pool.clone()));
        let exec_repo = Arc::new(SqliteExecutionRepository::new(pool.clone()));
        let exec_manager = Arc::new(ExecutionManager::new(max_concurrent));

        let (event_tx, event_rx) = mpsc::channel(100);
        let (scheduler, scheduler_cmd_rx) = Scheduler::new(event_tx);
        let scheduler_tx = scheduler.sender();

        // Load tasks into scheduler
        let enabled_tasks = task_repo.find_all_enabled().await?;
        for task in enabled_tasks {
            let _ = scheduler_tx.send(SchedulerCommand::add_task(task)).await;
        }

        // Spawn scheduler loop
        tokio::spawn(scheduler.run(scheduler_cmd_rx));

        let shutdown_notify = Arc::new(tokio::sync::Notify::new());

        let handler = Arc::new(AgentRpcHandler {
            task_repo: task_repo.clone(),
            exec_repo: exec_repo.clone(),
            exec_manager: exec_manager.clone(),
            scheduler_tx: scheduler_tx.clone(),
            start_time: Instant::now(),
            shutdown_notify: shutdown_notify.clone(),
        });

        let ipc_server = IpcServer::bind(ipc_path, handler).await?;

        Ok(Self {
            task_repo,
            exec_repo,
            exec_manager,
            scheduler_tx,
            event_rx,
            ipc_server,
            ipc_path: ipc_path.to_path_buf(),
            start_time: Instant::now(),
            shutdown_notify,
        })
    }

    pub async fn run(mut self) -> Result<()> {
        let event_tx = self.ipc_server.event_sender();
        let task_repo = self.task_repo.clone();
        let exec_repo = self.exec_repo.clone();
        let exec_manager = self.exec_manager.clone();

        // Spawn trigger event listener & execution dispatcher
        tokio::spawn(async move {
            while let Some(trigger_event) = self.event_rx.recv().await {
                let task = match task_repo.find_by_id(&trigger_event.task_id).await {
                    Ok(Some(t)) => t,
                    _ => continue,
                };

                let acquired = exec_manager
                    .try_acquire_slot(&task.id, task.execution_policy.concurrency_policy)
                    .await;
                if !acquired {
                    continue;
                }

                let t_repo = task_repo.clone();
                let e_repo = exec_repo.clone();
                let e_manager = exec_manager.clone();
                let ev_tx = event_tx.clone();

                tokio::spawn(async move {
                    let mut exec = Execution::new(task.id, trigger_event.trigger_id, Some(trigger_event.scheduled_at));
                    exec.status = ExecutionStatus::Running;
                    let _ = e_repo.create_run(&exec).await;

                    let _ = ev_tx.send(IpcEvent::new("execution.started", serde_json::json!({
                        "execution_id": exec.id,
                        "task_id": exec.task_id,
                    })));

                    let cancel = CancellationToken::new();
                    let mut final_status = ExecutionStatus::Succeeded;
                    let mut exit_code = Some(0);

                    for action in &task.actions {
                        if !action.enabled { continue; }
                        let res = ProcessRunner::run_action(
                            action,
                            task.working_directory.as_ref(),
                            &task.environment,
                            task.execution_policy.timeout_secs,
                            cancel.clone(),
                        ).await;

                        match res {
                            Ok(run_res) => {
                                if !run_res.stdout.is_empty() {
                                    let _ = e_repo.append_output(&exec.id, "stdout", &run_res.stdout).await;
                                    let _ = ev_tx.send(IpcEvent::new("execution.output", serde_json::json!({
                                        "execution_id": exec.id,
                                        "stream": "stdout",
                                        "content": run_res.stdout,
                                    })));
                                }
                                if !run_res.stderr.is_empty() {
                                    let _ = e_repo.append_output(&exec.id, "stderr", &run_res.stderr).await;
                                    let _ = ev_tx.send(IpcEvent::new("execution.output", serde_json::json!({
                                        "execution_id": exec.id,
                                        "stream": "stderr",
                                        "content": run_res.stderr,
                                    })));
                                }
                                exit_code = run_res.exit_code;
                                if run_res.status != ExecutionStatus::Succeeded {
                                    final_status = run_res.status;
                                    break;
                                }
                            }
                            Err(e) => {
                                final_status = ExecutionStatus::Failed;
                                break;
                            }
                        }
                    }

                    exec.status = final_status;
                    exec.finished_at = Some(chrono::Utc::now());
                    exec.exit_code = exit_code;
                    let _ = e_repo.update_run(&exec).await;

                    let _ = ev_tx.send(IpcEvent::new("execution.finished", serde_json::json!({
                        "execution_id": exec.id,
                        "status": exec.status,
                        "exit_code": exec.exit_code,
                    })));

                    e_manager.release_slot(&task.id).await;
                });
            }
        });

        // Run IPC server
        let shutdown = self.shutdown_notify.clone();
        tokio::select! {
            _ = self.ipc_server.run() => {}
            _ = shutdown.notified() => {
                info!("Agent service shutdown received");
            }
        }

        let _ = self.scheduler_tx.send(SchedulerCommand::Shutdown).await;
        Ok(())
    }
}

struct AgentRpcHandler {
    task_repo: Arc<SqliteTaskRepository>,
    exec_repo: Arc<SqliteExecutionRepository>,
    exec_manager: Arc<ExecutionManager>,
    scheduler_tx: mpsc::Sender<SchedulerCommand>,
    start_time: Instant,
    shutdown_notify: Arc<tokio::sync::Notify>,
}

#[async_trait]
impl RequestHandler for AgentRpcHandler {
    async fn handle_request(&self, req: IpcRequest) -> IpcResponse {
        match req.method.as_str() {
            "agent.status" => {
                let tasks = self.task_repo.find_all_enabled().await.unwrap_or_default();
                let status = AgentStatus {
                    version: "0.1.0".to_string(),
                    uptime_secs: self.start_time.elapsed().as_secs(),
                    active_tasks: tasks.len(),
                    running_executions: 0,
                };
                IpcResponse::success(req.id, serde_json::to_value(status).unwrap())
            }
            "agent.shutdown" => {
                self.shutdown_notify.notify_one();
                IpcResponse::success(req.id, serde_json::json!(true))
            }
            "task.list" => {
                match self.task_repo.find_all_enabled().await {
                    Ok(tasks) => IpcResponse::success(req.id, serde_json::to_value(tasks).unwrap()),
                    Err(e) => IpcResponse::error(req.id, e.to_string()),
                }
            }
            "task.get" => {
                let id: TaskId = match serde_json::from_value(req.params["id"].clone()) {
                    Ok(id) => id,
                    Err(e) => return IpcResponse::error(req.id, e.to_string()),
                };
                match self.task_repo.find_by_id(&id).await {
                    Ok(task) => IpcResponse::success(req.id, serde_json::to_value(task).unwrap()),
                    Err(e) => IpcResponse::error(req.id, e.to_string()),
                }
            }
            "task.save" => {
                let task: Task = match serde_json::from_value(req.params["task"].clone()) {
                    Ok(t) => t,
                    Err(e) => return IpcResponse::error(req.id, e.to_string()),
                };
                match self.task_repo.save(&task).await {
                    Ok(()) => {
                        let _ = self.scheduler_tx.send(SchedulerCommand::add_task(task.clone())).await;
                        IpcResponse::success(req.id, serde_json::to_value(task).unwrap())
                    }
                    Err(e) => IpcResponse::error(req.id, e.to_string()),
                }
            }
            "task.delete" => {
                let id: TaskId = match serde_json::from_value(req.params["id"].clone()) {
                    Ok(id) => id,
                    Err(e) => return IpcResponse::error(req.id, e.to_string()),
                };
                match self.task_repo.delete(&id).await {
                    Ok(()) => {
                        let _ = self.scheduler_tx.send(SchedulerCommand::RemoveTask(id)).await;
                        IpcResponse::success(req.id, serde_json::json!(true))
                    }
                    Err(e) => IpcResponse::error(req.id, e.to_string()),
                }
            }
            "task.trigger_now" => {
                let id: TaskId = match serde_json::from_value(req.params["id"].clone()) {
                    Ok(id) => id,
                    Err(e) => return IpcResponse::error(req.id, e.to_string()),
                };
                let _ = self.scheduler_tx.send(SchedulerCommand::TriggerNow(id)).await;
                IpcResponse::success(req.id, serde_json::json!(true))
            }
            _ => IpcResponse::error(req.id, format!("Method '{}' not found", req.method)),
        }
    }
}
```

Update `apps/agent/src/lib.rs`:
```rust
pub mod lock;
pub mod service;
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p easyjob-agent --test service_tests`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add apps/agent
git commit -m "feat(agent): implement AgentService linking core engine with IPC request handler and event pipeline"
```

---

### Task 6: Agent Binary CLI & Full Daemon End-to-End Tests

**Files:**
- Create: `apps/agent/src/main.rs`
- Create: `tests/agent_daemon_e2e_test.rs`
- Modify: `Cargo.toml`

- [ ] **Step 1: Write `apps/agent/src/main.rs`**

```rust
use clap::Parser;
use easyjob_agent::lock::{LockOutcome, SingleInstanceLock};
use easyjob_agent::service::AgentService;
use easyjob_ipc::default_ipc_path;
use easyjob_logging::init_logging;
use std::path::PathBuf;
use tracing::{error, info};

#[derive(Parser, Debug)]
#[command(author, version, about = "easyJob Background Task Scheduling Daemon")]
struct Cli {
    /// Data directory containing database, logs, and sockets
    #[arg(long)]
    data_dir: Option<PathBuf>,

    /// Run in daemon mode (suppress console stdout logging)
    #[arg(long, default_value_t = false)]
    daemon: bool,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    let data_dir = cli.data_dir.unwrap_or_else(|| {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
        PathBuf::from(home).join(".easyjob")
    });
    std::fs::create_dir_all(&data_dir)?;

    let log_dir = data_dir.join("logs");
    let _guard = init_logging(&log_dir, !cli.daemon, "info")?;

    let lock_path = data_dir.join("agent.lock");
    let ipc_path = data_dir.join("easyjob.sock");
    let db_path = data_dir.join("easyjob.db");
    let db_url = format!("sqlite://{}?mode=rwc", db_path.to_string_lossy());

    match SingleInstanceLock::acquire(&lock_path, &ipc_path).await? {
        LockOutcome::AlreadyRunning => {
            println!("easyjob-agent is already running.");
            return Ok(());
        }
        LockOutcome::Acquired(_lock) => {
            info!("Starting easyJob Agent Daemon v0.1.0");
            let service = AgentService::init(&db_url, &ipc_path, 16).await?;

            tokio::select! {
                res = service.run() => {
                    if let Err(e) = res {
                        error!("Agent service encountered error: {:?}", e);
                    }
                }
                _ = tokio::signal::ctrl_c() => {
                    info!("Received SIGINT/Ctrl+C, exiting gracefully");
                }
            }
        }
    }

    Ok(())
}
```

- [ ] **Step 2: Write E2E Daemon Test in `tests/agent_daemon_e2e_test.rs`**

```rust
use easyjob_agent::service::AgentService;
use easyjob_common::{ActionId, TaskId, TriggerId};
use easyjob_domain::action::{Action, ActionKind};
use easyjob_domain::policy::ExecutionPolicy;
use easyjob_domain::task::Task;
use easyjob_domain::trigger::{Trigger, TriggerKind};
use easyjob_ipc::client::IpcClient;
use std::collections::HashMap;
use tempfile::tempdir;

#[tokio::test]
async fn test_full_agent_daemon_ipc_lifecycle() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("daemon.db");
    let socket_path = dir.path().join("daemon.sock");
    let db_url = format!("sqlite://{}?mode=rwc", db_path.to_string_lossy());

    let service = AgentService::init(&db_url, &socket_path, 4).await.unwrap();
    let service_handle = tokio::spawn(service.run());

    let client = IpcClient::connect(&socket_path).await.unwrap();
    let mut event_rx = client.subscribe();

    // 1. Check Agent Status
    let status = client.call("agent.status", serde_json::json!({})).await.unwrap();
    assert_eq!(status["version"], "0.1.0");

    // 2. Save a Task via IPC
    let task_id = TaskId::new();
    let task = Task {
        id: task_id,
        name: "IPC Daemon Job".to_string(),
        description: None,
        enabled: true,
        triggers: vec![Trigger {
            id: TriggerId::new(),
            task_id,
            enabled: true,
            kind: TriggerKind::AgentStarted,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        }],
        actions: vec![Action {
            id: ActionId::new(),
            task_id,
            sequence: 1,
            enabled: true,
            kind: ActionKind::ExecuteShell { command: "echo 'daemon IPC live'".to_string() },
        }],
        execution_policy: ExecutionPolicy::default(),
        working_directory: None,
        environment: HashMap::new(),
        version: 1,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    };

    let saved = client.call("task.save", serde_json::json!({ "task": task })).await.unwrap();
    assert_eq!(saved["name"], "IPC Daemon Job");

    // 3. Trigger Job Now
    client.call("task.trigger_now", serde_json::json!({ "id": task_id })).await.unwrap();

    // 4. Await Execution Events over IPC broadcast
    let started_ev = tokio::time::timeout(std::time::Duration::from_secs(3), event_rx.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(started_ev.event, "execution.started");

    // 5. Shutdown Daemon
    client.call("agent.shutdown", serde_json::json!({})).await.unwrap();
    let _ = service_handle.await;
}
```

- [ ] **Step 3: Run all workspace tests**

Run: `cargo test --all`
Expected: ALL PASS

- [ ] **Step 4: Commit**

```bash
git add apps/agent tests/agent_daemon_e2e_test.rs
git commit -m "feat(agent): implement easyjob-agent main binary and full daemon e2e test suite"
```
