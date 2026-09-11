# easyJob Phase 2 Local IPC & Agent Service Design Specification

## 1. Overview & Goals

This document specifies the technical design for **Phase 2: Local IPC & Agent Service** of easyJob.
Phase 2 turns the headless Phase 1 Core Engine into an independent, long-running desktop daemon (`apps/agent`), equipped with:
- Structured Diagnostic Logging (`crates/logging`)
- Bidirectional Local IPC Protocol and Cross-Platform Transport (`crates/ipc`)
- Single-Instance File Locking and Stale Lock Recovery (`agent.lock`)
- Lifecycle Orchestration & Event Dispatching Pipeline

### Core Architectural Principles
1. **Zero Shared Memory with UI**: The GUI (Tauri Desktop) and Agent Daemon run in isolated OS processes, communicating strictly over local IPC.
2. **Native OS IPC Channels**: Unix Domain Sockets on macOS/Linux and Named Pipes on Windows, avoiding open network ports and local network port collision/security vulnerabilities.
3. **Robust Single-Instance Management**: An exclusive file lock (`agent.lock`) with IPC liveness probing ensures that only one Agent instance runs per user account while safely recovering from abrupt crashes.
4. **Bi-directional Event Streaming**: Real-time push for task lifecycle events (`execution.started`, `execution.output`, `execution.finished`) alongside standard Request/Response RPC methods.

---

## 2. Workspace & Module Organization

Updated Cargo workspace structure:

```text
easyJob/
├── Cargo.toml
├── docs/
│   ├── eastJob-architecture-design.md
│   ├── superpowers/specs/2026-09-11-easyjob-core-engine-design.md
│   ├── superpowers/specs/2026-09-11-easyjob-ipc-agent-design.md
│   └── superpowers/plans/2026-09-11-easyjob-core-engine.md
├── apps/
│   └── agent/                     # easyjob-agent binary executable
├── crates/
│   ├── common/
│   ├── domain/
│   ├── platform/
│   ├── executor/
│   ├── scheduler/
│   ├── persistence/
│   ├── ipc/                       # Protocol, server, client, transport
│   └── logging/                   # tracing subscriber & rolling appender
└── migrations/
```

### Crate Dependencies for Phase 2

```text
               crates/common
                     ▲
         ┌───────────┼───────────┐
         │           │           │
    crates/domain    │           │
         ▲           │           │
   ┌─────┴──────┐    │           │
   │            │    │           │
crates/      crates/ │           │
scheduler    executor│           │
   │            │    │           │
   │            ▼    │           │
   │     crates/platform         │
   │            │                │
   └────────┬───┴────────────────┘
            │
    crates/persistence
            │
            ├───────────────┐
            ▼               ▼
       crates/ipc     crates/logging
            ▲               ▲
            └───────┬───────┘
                    │
               apps/agent (binary)
```

---

## 3. Detailed Component Designs

### 3.1 IPC Protocol & Types (`crates/ipc`)

#### Wire Format
- Newline-delimited JSON (`\n`), handled via `tokio_util::codec::LinesCodec`.
- Encodings: UTF-8. Maximum message size: 4MB.

#### Message Models
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IpcRequest {
    pub id: String,
    pub method: String,
    pub params: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IpcResponse {
    pub id: String,
    pub ok: bool,
    pub data: Option<serde_json::Value>,
    pub error: Option<String>,
}

impl IpcResponse {
    pub fn success(id: String, data: serde_json::Value) -> Self {
        Self { id, ok: true, data: Some(data), error: None }
    }

    pub fn error(id: String, msg: impl Into<String>) -> Self {
        Self { id, ok: false, data: None, error: Some(msg.into()) }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IpcEvent {
    pub event: String,
    pub data: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum IpcMessage {
    Request(IpcRequest),
    Response(IpcResponse),
    Event(IpcEvent),
}
```

#### RPC Methods & Payloads
| Method | Params | Return / Data | Description |
|---|---|---|---|
| `task.list` | `{}` | `Vec<Task>` | List all tasks |
| `task.get` | `{ "id": TaskId }` | `Option<Task>` | Get task details |
| `task.save` | `{ "task": Task }` | `Task` | Save task and update scheduler queue |
| `task.delete` | `{ "id": TaskId }` | `bool` | Cascade delete task and invalidate in scheduler |
| `task.trigger_now` | `{ "id": TaskId }` | `bool` | Trigger immediate task execution |
| `execution.list` | `{ "limit": u32 }` | `Vec<Execution>` | Retrieve recent execution runs |
| `execution.get` | `{ "id": ExecutionId }` | `Option<Execution>` | Retrieve execution details |
| `execution.cancel` | `{ "id": ExecutionId }` | `bool` | Cancel actively running execution |
| `agent.status` | `{}` | `AgentStatus` | Status (uptime, tasks, running, version) |
| `agent.shutdown` | `{}` | `bool` | Gracefully terminate the Agent daemon |

#### Broadcast Events
- `execution.started`: `{ "execution_id": "...", "task_id": "...", "started_at": "..." }`
- `execution.output`: `{ "execution_id": "...", "stream": "stdout"|"stderr", "content": "..." }`
- `execution.finished`: `{ "execution_id": "...", "status": "...", "exit_code": ..., "duration_ms": ... }`

---

### 3.2 Transport Layer: Cross-Platform IPC (`crates/ipc`)

#### Paths & Addressing
- Default Socket / Pipe Path:
  - macOS / Linux: `~/.easyjob/easyjob.sock`
  - Windows: `\\.\pipe\easyjob-<current_username>`

#### Server (`IpcServer`)
- **Unix Domain Socket**:
  - Automatically unlinks dead socket files if prior agent crashed.
  - Sets file mode to `0600` via `std::os::unix::fs::PermissionsExt`.
  - Spawns per-client connection tasks.
- **Windows Named Pipe**:
  - Configures `ServerOptions::new().first_pipe_instance(true)` on initialization to assert pipe ownership.
  - Successively spawns client connection handlers on new instances.
- **Client Session Manager**:
  - Maintains `Arc<Mutex<HashMap<u64, mpsc::Sender<IpcMessage>>>>`.
  - Broadcasts events to all active clients; dead channels are automatically removed on error.

#### Client SDK (`IpcClient`)
- Simple asynchronous client for Tauri / integration testing:
  - `client.call("method", params).await -> Result<serde_json::Value>`
  - `client.events() -> broadcast::Receiver<IpcEvent>`

---

### 3.3 Logging Architecture (`crates/logging`)

- System logs written to `~/.easyjob/logs/agent.log`.
- Log rolling: `tracing_appender::rolling::daily("~/.easyjob/logs", "agent.log")`.
- Console output: formatted with ANSI colors when connected to a terminal / `is_terminal()`.
- Filter: configured via `RUST_LOG` (defaults to `info`).

---

### 3.4 Agent Daemon & Lifecycle (`apps/agent`)

#### Single-Instance File Lock (`agent.lock`)
- Path: `~/.easyjob/agent.lock`.
- Uses `fs2::FileExt::try_lock_exclusive`.
- If lock is held:
  - Connects to existing IPC socket and issues `agent.status`.
  - If agent responds: prints `"easyjob-agent is already running (version ...)"` and exits `0`.
  - If connection fails / timeout: deletes stale lock file, re-acquires lock, logs `"Stale lock recovered"`.

#### Orchestration Pipeline
```text
1. Initialize tracing log subscriber.
2. Acquire agent.lock.
3. Open SQLite DB pool & run migrations.
4. Call recover_dangling_executions (reset Running -> Interrupted).
5. Load enabled tasks from TaskRepository.
6. Initialize Scheduler, ScheduleQueue, and ExecutionManager.
7. Push loaded tasks into Scheduler queue.
8. Spawn Scheduler::run loop in Tokio task.
9. Spawn Event Dispatcher:
   - Listens to TriggerEvent from Scheduler.
   - Queries task -> checks ExecutionManager::try_acquire_slot.
   - Inserts Execution record into DB, broadcasts execution.started.
   - Executes with ProcessRunner.
   - Streams stdout/stderr -> appends to run_outputs & broadcasts execution.output.
   - Updates Execution record in DB, broadcasts execution.finished.
   - Releases slot on ExecutionManager.
10. Spawn IpcServer, route incoming RPC requests to TaskRepository, Scheduler, ExecutionManager.
11. Await SIGINT / SIGTERM / agent.shutdown:
    - Send SchedulerCommand::Shutdown.
    - Close IPC listener.
    - Release lock & cleanup socket.
```

---

## 4. Verification Plan

### Automated Tests
1. **IPC Protocol Serialization Tests** (`crates/ipc`):
   - Roundtrip serialization for `IpcRequest`, `IpcResponse`, `IpcEvent`, and `IpcMessage`.
2. **IPC Transport & Client-Server Tests** (`crates/ipc`):
   - Client connects to `IpcServer` over UDS (or Named Pipe on Windows).
   - RPC request/response handling (`call` returns matching response).
   - Event broadcast delivered to multiple connected clients.
3. **File Lock & Single Instance Tests** (`apps/agent`):
   - Exclusive lock acquisition and double-lock prevention.
   - Stale lock detection and recovery.
4. **End-to-End Daemon Tests** (`apps/agent`):
   - Start full Agent daemon against temporary app directory.
   - Connect `IpcClient`:
     - Issue `task.save` -> verify in DB and scheduler.
     - Issue `task.trigger_now` -> verify `execution.started`, `execution.output`, and `execution.finished` events received over IPC.
     - Issue `agent.status` -> verify uptime and status report.
     - Issue `agent.shutdown` -> verify graceful exit and socket cleanup.
