# easyJob Phase 1 Core Engine Design Specification

## 1. Overview & Goals

easyJob is a cross-platform desktop automation task scheduler running on macOS and Windows.
This document specifies the technical design for **Phase 1: Rust Core Engine**, which encompasses:
- Pure Domain Models (`crates/domain`, `crates/common`)
- Trigger Engine & Scheduler Priority Queue (`crates/scheduler`)
- Process Execution & Cross-Platform Management (`crates/executor`, `crates/platform`)
- SQLite Persistence & Automatic Migrations (`crates/persistence`)

### Core Architectural Principles
1. **Separation of Concerns**: Domain logic is purely functional and testable without SQLite, Tauri, or OS APIs.
2. **Dual Clock System**: Calendar calculations rely on Wall Clock with explicit timezones; sleep intervals rely on Monotonic Clocks (`tokio::time::Instant`).
3. **Robust Isolation**: The Scheduler never blocks on long-running tasks. Process execution runs asynchronously via event channels.
4. **Cross-Platform Silent Execution**: Process groups on macOS (`pgid`) and Job Objects with `CREATE_NO_WINDOW` on Windows prevent orphaned processes and flashing command windows.
5. **Persistence with WAL**: SQLite in WAL mode ensures reliable concurrency, transaction boundaries, and crash recovery.

---

## 2. Workspace & Module Organization

A Cargo workspace structured as follows:

```text
easyJob/
├── Cargo.toml
├── docs/
│   ├── eastJob-architecture-design.md
│   └── superpowers/specs/2026-09-11-easyjob-core-engine-design.md
├── crates/
│   ├── common/
│   ├── domain/
│   ├── scheduler/
│   ├── executor/
│   ├── platform/
│   └── persistence/
└── migrations/
    └── 20260911000000_init_schema.sql
```

### Crate Dependencies

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
```

---

## 3. Detailed Component Designs

### 3.1 Domain Model (`crates/domain`)

#### Identifiers
- Strongly-typed UUID v4 wrappers: `TaskId`, `TriggerId`, `ActionId`, `ExecutionId`.

#### Task Aggregate
```rust
pub struct Task {
    pub id: TaskId,
    pub name: String,
    pub description: Option<String>,
    pub enabled: bool,
    pub triggers: Vec<Trigger>,
    pub actions: Vec<Action>,
    pub execution_policy: ExecutionPolicy,
    pub working_directory: Option<PathBuf>,
    pub environment: HashMap<String, String>,
    pub version: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
```

#### Triggers
```rust
pub struct Trigger {
    pub id: TriggerId,
    pub task_id: TaskId,
    pub enabled: bool,
    pub kind: TriggerKind,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

pub enum TriggerKind {
    Once {
        fire_at: DateTime<Utc>,
    },
    Daily {
        time: NaiveTime,
        timezone: String, // e.g. "Asia/Shanghai"
    },
    Weekly {
        days_of_week: Vec<Weekday>,
        time: NaiveTime,
        timezone: String,
    },
    Interval {
        interval_secs: u64,
        start_at: Option<DateTime<Utc>>,
    },
    AgentStarted,
}
```

#### Actions
```rust
pub struct Action {
    pub id: ActionId,
    pub task_id: TaskId,
    pub sequence: u32,
    pub enabled: bool,
    pub kind: ActionKind,
}

pub enum ActionKind {
    ExecuteProgram {
        program: String,
        args: Vec<String>,
    },
    ExecuteShell {
        command: String,
    },
    ExecutePowerShell {
        script: String,
        no_profile: bool,
    },
    ExecuteCmd {
        command: String,
    },
}
```

#### Policies & Execution
```rust
pub struct ExecutionPolicy {
    pub concurrency_policy: ConcurrencyPolicy,
    pub missed_run_policy: MissedRunPolicy,
    pub retry_policy: RetryPolicy,
    pub timeout_secs: Option<u64>,
}

pub enum ConcurrencyPolicy {
    AllowParallel,
    SkipIfRunning,
    QueueOne,
}

pub enum MissedRunPolicy {
    RunOnce,
    Skip,
}

pub struct RetryPolicy {
    pub max_retries: u32,
    pub delay_secs: u64,
}

pub enum ExecutionStatus {
    Queued,
    Running,
    Succeeded,
    Failed,
    TimedOut,
    Cancelled,
    Skipped,
}

pub struct Execution {
    pub id: ExecutionId,
    pub task_id: TaskId,
    pub trigger_id: Option<TriggerId>,
    pub status: ExecutionStatus,
    pub scheduled_at: Option<DateTime<Utc>>,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
    pub duration_ms: Option<u64>,
    pub exit_code: Option<i32>,
    pub error_message: Option<String>,
}
```

---

### 3.2 Trigger Engine & Scheduler (`crates/scheduler`)

#### Trigger Evaluator Trait
```rust
pub trait TriggerEvaluator: Send + Sync {
    fn next_occurrence(&self, after: DateTime<Utc>) -> Option<DateTime<Utc>>;
}
```

- **Daily / Weekly**: Resolves local datetime in target timezone using `chrono-tz`, then converts to `DateTime<Utc>`. If local time doesn't exist due to DST gap, shifts forward to valid local time.
- **Interval**: Strictly adds `interval_secs` from `start_at` or current iteration.
- **Once**: Returns `fire_at` if `fire_at > after`, otherwise `None`.

#### Priority Queue & Min-Heap
```rust
#[derive(Eq, PartialEq)]
pub struct ScheduledItem {
    pub task_id: TaskId,
    pub trigger_id: TriggerId,
    pub next_fire_at: DateTime<Utc>,
    pub generation: u64,
}

impl Ord for ScheduledItem {
    fn cmp(&self, other: &Self) -> Ordering {
        // Reverse for Min-Heap
        other.next_fire_at.cmp(&self.next_fire_at)
            .then_with(|| self.task_id.cmp(&other.task_id))
    }
}
```

#### Scheduler Main Event Loop
The scheduler runs a Tokio loop selecting over:
1. `tokio::time::sleep_until(next_deadline)`: Pop earliest item, emit trigger event to `ExecutionManager`, compute next occurrence and re-insert.
2. `command_rx.recv()`:
   - `AddTask(Task)`: Calculate next occurrences for its triggers, push to queue.
   - `UpdateTask(Task)`: Bump task generation, invalidate existing items in queue, push new triggers.
   - `RemoveTask(TaskId)`: Invalidate generation for task.
   - `TriggerNow(TaskId)`: Immediately dispatch task execution regardless of schedule.
3. `clock_tick`: Every 10 seconds, checks `|Utc::now() - expected_wall_clock| > 30s`. If jump detected, reloads enabled triggers and rebuilds priority queue.

---

### 3.3 Platform & Process Management (`crates/platform`, `crates/executor`)

#### Platform Process Adapter (`crates/platform`)
- **macOS / Unix**:
  - Shell: `/bin/zsh -c` or `/bin/sh -c`.
  - Process Group: In `Command::pre_exec`, calls `libc::setpgid(0, 0)` so child becomes process group leader.
  - Tree Kill: Sends `SIGTERM` to `-pgid`, waits 3 seconds, then sends `SIGKILL` to `-pgid` if still running.
- **Windows**:
  - Shell: `cmd.exe /c` or `powershell.exe -NoProfile -NonInteractive -ExecutionPolicy Bypass -Command`.
  - Silent Window: `creation_flags(0x08000000)` (`CREATE_NO_WINDOW`).
  - Tree Kill: Assigns process to Windows Job Object with kill-on-close, or invokes `TerminateJobObject`.

#### ProcessManager & Output Streaming (`crates/executor`)
- Asynchronously reads `stdout` and `stderr` chunks using `tokio::io::AsyncReadExt`.
- Caps max captured stream size (e.g. 2MB) with truncation notification `\n... [output truncated]`.
- Implements `timeout(timeout_secs)`: sends termination signal upon timeout, marking execution as `TimedOut`.
- Listens to `CancellationToken` for user manual cancellations.

#### ExecutionManager
- Manages active runs and enforces concurrency policies:
  - `AllowParallel`: Dispatches immediately.
  - `SkipIfRunning`: Drops trigger and records `Skipped` if task has a running execution.
  - `QueueOne`: Holds one pending execution; dispatches immediately when current execution finishes.
- Global concurrency limit enforced with `tokio::sync::Semaphore` (configurable, default 16).

---

### 3.4 Persistence (`crates/persistence`)

#### SQLite Configuration
- Connection string: `sqlite://easyjob.db?mode=rwc`
- PRAGMAs:
  - `journal_mode = WAL`
  - `foreign_keys = ON`
  - `busy_timeout = 5000`
  - `synchronous = NORMAL`

#### Migrations Schema
- `tasks`: Table for task definitions and metadata.
- `triggers`: Table with foreign key to tasks.
- `actions`: Table with ordered sequence and foreign key to tasks.
- `task_runs`: Historical record of each execution.
- `run_outputs`: Log streams associated with runs.
- `agent_state`: Key-value runtime recovery state.
- `settings`: Global application settings.

#### Repositories
- `TaskRepository`: CRUD operations with atomic SQLite transactions for Task + Triggers + Actions.
- `ExecutionRepository`: Insert and update execution statuses, append output chunks, query execution history.
- `Recovery`: On startup, updates any dangling `Running` executions to `Interrupted`.

---

## 4. Error Handling & Edge Cases

| Scenario | Behavior |
|---|---|
| Machine sleeps & wakes up | Monotonic timer resumes, clock jump detector spots wall clock deviation, triggers queue rebuild, applies `MissedRunPolicy` (`RunOnce` vs `Skip`). |
| Process hangs indefinitely | Action `timeout_secs` expires -> graceful termination -> force kill -> status set to `TimedOut`. |
| Massive stdout/stderr flood | Captured buffer truncated at 2MB, output preserved safely without OOM or DB lock contention. |
| Corrupt task definition | Validation at repository layer rejects invalid cron/interval, logs error without halting scheduler. |
| Agent crashes mid-run | On next launch, recovery scanner detects unclosed `Running` runs and marks them `Interrupted`. |

---

## 5. Verification Plan

### Automated Tests
1. **Domain Unit Tests** (`crates/domain`):
   - Serialization / deserialization of Task, Trigger, Action configurations.
   - Validation logic for execution policies.
2. **Trigger Engine Tests** (`crates/scheduler`):
   - `Daily`, `Weekly`, `Interval`, `Once` next fire calculations across timezones (UTC, Asia/Shanghai, US/Pacific).
   - DST jump and transition calculations.
3. **Scheduler Integration Tests** (`crates/scheduler`):
   - MinHeap ordering and generation invalidation.
   - Dynamic task insertion, editing, and cancellation.
   - Missed run policy behavior (`RunOnce` vs `Skip`).
4. **Executor & Platform Tests** (`crates/executor`, `crates/platform`):
   - Echo command stdout capture and verification.
   - Non-zero exit code error handling.
   - Timeout and process group kill verification.
   - Concurrency policy verification (`SkipIfRunning`, `QueueOne`).
5. **Persistence Tests** (`crates/persistence`):
   - SQLite migration run against in-memory or temp db.
   - Atomic task save and cascade delete.
   - Execution status updates and crash recovery scan.
