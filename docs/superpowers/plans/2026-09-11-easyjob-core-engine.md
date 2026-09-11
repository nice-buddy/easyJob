# easyJob Phase 1: Core Engine Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build and verify the complete headless Phase 1 Rust core engine for easyJob: Cargo workspace, Domain models, Trigger Engine, Min-Heap Scheduler, Platform process adapter, Process Executor with concurrency control, and SQLite WAL persistence with automatic migrations.

**Architecture:** A multi-crate Rust Cargo workspace. `domain` contains purely functional models; `scheduler` calculates triggers and maintains a priority queue driven by Tokio timers; `platform` manages OS-level process trees and silent execution; `executor` streams output and enforces concurrency policies; `persistence` provides atomic SQLite WAL operations via `sqlx`.

**Tech Stack:** Rust (edition 2021, rustc 1.92+), Tokio (async runtime), sqlx (SQLite WAL), chrono & chrono-tz (time & timezones), uuid, serde, thiserror, libc (macOS process groups), windows-sys (Windows Job Objects & CREATE_NO_WINDOW).

## Global Constraints

- Domain crate MUST NOT depend on SQLite, Tokio, or OS APIs.
- External commands MUST execute silently (no popup command prompt window on Windows, stdout/stderr piped).
- Output stream buffer per execution MUST NOT exceed 2MB; excess content truncated with `\n... [output truncated]`.
- Database MUST operate with `PRAGMA journal_mode = WAL;` and foreign keys enabled.
- All errors MUST be typed via `thiserror` in `crates/common`.
- Every task MUST conclude with passing automated tests and a git commit.

---

### Task 1: Workspace Scaffolding & `crates/common`

**Files:**
- Create: `Cargo.toml`
- Create: `crates/common/Cargo.toml`
- Create: `crates/common/src/lib.rs`
- Create: `crates/common/src/id.rs`
- Create: `crates/common/src/error.rs`
- Create: `crates/common/src/time.rs`
- Test: `crates/common/tests/id_tests.rs`

**Interfaces:**
- Consumes: None
- Produces:
  - `common::id::{TaskId, TriggerId, ActionId, ExecutionId}`
  - `common::error::{Error, Result}`
  - `common::time::{now_utc, duration_between_utc}`

- [ ] **Step 1: Write root `Cargo.toml` and `crates/common/Cargo.toml`**

`Cargo.toml`:
```toml
[workspace]
members = [
    "crates/common",
    "crates/domain",
    "crates/platform",
    "crates/executor",
    "crates/scheduler",
    "crates/persistence",
]
resolver = "2"

[workspace.dependencies]
chrono = { version = "0.4.38", features = ["serde"] }
chrono-tz = { version = "0.10.0", features = ["serde"] }
uuid = { version = "1.10.0", features = ["v4", "serde"] }
serde = { version = "1.0.210", features = ["derive"] }
serde_json = "1.0.128"
thiserror = "1.0.64"
tokio = { version = "1.40.0", features = ["full"] }
tokio-util = "0.7.12"
async-trait = "0.1.83"
tracing = "0.1.40"
sqlx = { version = "0.8.2", features = ["runtime-tokio", "sqlite", "chrono", "uuid", "migrate"] }
```

`crates/common/Cargo.toml`:
```toml
[package]
name = "easyjob-common"
version = "0.1.0"
edition = "2021"

[dependencies]
uuid.workspace = true
serde.workspace = true
serde_json.workspace = true
thiserror.workspace = true
chrono.workspace = true
```

- [ ] **Step 2: Write failing test in `crates/common/tests/id_tests.rs`**

```rust
use easyjob_common::id::{TaskId, TriggerId, ActionId, ExecutionId};

#[test]
fn test_id_generation_and_serde() {
    let task_id = TaskId::new();
    let json = serde_json::to_string(&task_id).unwrap();
    let deserialized: TaskId = serde_json::from_str(&json).unwrap();
    assert_eq!(task_id, deserialized);
    assert!(!task_id.to_string().is_empty());

    let trigger_id = TriggerId::new();
    let action_id = ActionId::new();
    let exec_id = ExecutionId::new();
    assert_ne!(trigger_id.to_string(), action_id.to_string());
    assert_ne!(action_id.to_string(), exec_id.to_string());
}
```

- [ ] **Step 3: Run test to verify it fails**

Run: `cargo test -p easyjob-common --test id_tests`
Expected: FAIL (types and crate not implemented yet)

- [ ] **Step 4: Implement `crates/common` modules**

`crates/common/src/id.rs`:
```rust
use serde::{Deserialize, Serialize};
use std::fmt;
use uuid::Uuid;

macro_rules! define_id {
    ($name:ident) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
        pub struct $name(pub Uuid);

        impl $name {
            pub fn new() -> Self {
                Self(Uuid::new_v4())
            }

            pub fn from_uuid(uuid: Uuid) -> Self {
                Self(uuid)
            }

            pub fn parse(s: &str) -> Result<Self, uuid::Error> {
                Ok(Self(Uuid::parse_str(s)?))
            }

            pub fn as_uuid(&self) -> &Uuid {
                &self.0
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}", self.0)
            }
        }
    };
}

define_id!(TaskId);
define_id!(TriggerId);
define_id!(ActionId);
define_id!(ExecutionId);
```

`crates/common/src/error.rs`:
```rust
use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error("Validation error: {0}")]
    Validation(String),

    #[error("Not found: {0}")]
    NotFound(String),

    #[error("Process execution error: {0}")]
    Process(String),

    #[error("Scheduler error: {0}")]
    Scheduler(String),

    #[error("Database error: {0}")]
    Database(String),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Unknown error: {0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, Error>;
```

`crates/common/src/time.rs`:
```rust
use chrono::{DateTime, Utc};

pub fn now_utc() -> DateTime<Utc> {
    Utc::now()
}

pub fn duration_ms_between(start: DateTime<Utc>, end: DateTime<Utc>) -> u64 {
    (end - start).num_milliseconds().max(0) as u64
}
```

`crates/common/src/lib.rs`:
```rust
pub mod error;
pub mod id;
pub mod time;

pub use error::{Error, Result};
pub use id::{ActionId, ExecutionId, TaskId, TriggerId};
```

- [ ] **Step 5: Run test to verify it passes**

Run: `cargo test -p easyjob-common`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml crates/common
git commit -m "feat(common): add workspace setup and common id, error, time modules"
```

---

### Task 2: Pure Domain Models (`crates/domain`)

**Files:**
- Create: `crates/domain/Cargo.toml`
- Create: `crates/domain/src/lib.rs`
- Create: `crates/domain/src/task.rs`
- Create: `crates/domain/src/trigger.rs`
- Create: `crates/domain/src/action.rs`
- Create: `crates/domain/src/execution.rs`
- Create: `crates/domain/src/policy.rs`
- Test: `crates/domain/tests/domain_tests.rs`

**Interfaces:**
- Consumes: `easyjob_common::{TaskId, TriggerId, ActionId, ExecutionId, Error, Result}`
- Produces:
  - `domain::task::Task`
  - `domain::trigger::{Trigger, TriggerKind}`
  - `domain::action::{Action, ActionKind}`
  - `domain::execution::{Execution, ExecutionStatus}`
  - `domain::policy::{ExecutionPolicy, ConcurrencyPolicy, MissedRunPolicy, RetryPolicy}`

- [ ] **Step 1: Write `crates/domain/Cargo.toml`**

```toml
[package]
name = "easyjob-domain"
version = "0.1.0"
edition = "2021"

[dependencies]
easyjob-common = { path = "../common" }
chrono.workspace = true
chrono-tz.workspace = true
serde.workspace = true
serde_json.workspace = true
uuid.workspace = true
```

- [ ] **Step 2: Write failing test in `crates/domain/tests/domain_tests.rs`**

```rust
use chrono::{NaiveTime, Utc, Weekday};
use easyjob_common::{TaskId, TriggerId, ActionId};
use easyjob_domain::{
    action::{Action, ActionKind},
    policy::{ConcurrencyPolicy, ExecutionPolicy, MissedRunPolicy, RetryPolicy},
    task::Task,
    trigger::{Trigger, TriggerKind},
};
use std::collections::HashMap;

#[test]
fn test_task_creation_and_serialization() {
    let task_id = TaskId::new();
    let trigger = Trigger {
        id: TriggerId::new(),
        task_id,
        enabled: true,
        kind: TriggerKind::Daily {
            time: NaiveTime::from_hms_opt(8, 30, 0).unwrap(),
            timezone: "Asia/Shanghai".to_string(),
        },
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };
    let action = Action {
        id: ActionId::new(),
        task_id,
        sequence: 1,
        enabled: true,
        kind: ActionKind::ExecuteShell {
            command: "echo hello".to_string(),
        },
    };
    let task = Task {
        id: task_id,
        name: "Morning Backup".to_string(),
        description: Some("Daily automated job".to_string()),
        enabled: true,
        triggers: vec![trigger],
        actions: vec![action],
        execution_policy: ExecutionPolicy {
            concurrency_policy: ConcurrencyPolicy::SkipIfRunning,
            missed_run_policy: MissedRunPolicy::RunOnce,
            retry_policy: RetryPolicy { max_retries: 2, delay_secs: 5 },
            timeout_secs: Some(300),
        },
        working_directory: None,
        environment: HashMap::new(),
        version: 1,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };

    let serialized = serde_json::to_string(&task).unwrap();
    let deserialized: Task = serde_json::from_str(&serialized).unwrap();
    assert_eq!(task.id, deserialized.id);
    assert_eq!(task.name, deserialized.name);
    assert_eq!(deserialized.triggers.len(), 1);
    assert_eq!(deserialized.actions.len(), 1);
}
```

- [ ] **Step 3: Run test to verify it fails**

Run: `cargo test -p easyjob-domain --test domain_tests`
Expected: FAIL

- [ ] **Step 4: Implement Domain types**

`crates/domain/src/policy.rs`:
```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConcurrencyPolicy {
    AllowParallel,
    SkipIfRunning,
    QueueOne,
}

impl Default for ConcurrencyPolicy {
    fn default() -> Self {
        Self::SkipIfRunning
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MissedRunPolicy {
    RunOnce,
    Skip,
}

impl Default for MissedRunPolicy {
    fn default() -> Self {
        Self::RunOnce
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RetryPolicy {
    pub max_retries: u32,
    pub delay_secs: u64,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_retries: 0,
            delay_secs: 0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ExecutionPolicy {
    pub concurrency_policy: ConcurrencyPolicy,
    pub missed_run_policy: MissedRunPolicy,
    pub retry_policy: RetryPolicy,
    pub timeout_secs: Option<u64>,
}
```

`crates/domain/src/trigger.rs`:
```rust
use chrono::{DateTime, NaiveTime, Utc, Weekday};
use easyjob_common::{TaskId, TriggerId};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TriggerKind {
    Once {
        fire_at: DateTime<Utc>,
    },
    Daily {
        time: NaiveTime,
        timezone: String,
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Trigger {
    pub id: TriggerId,
    pub task_id: TaskId,
    pub enabled: bool,
    pub kind: TriggerKind,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
```

`crates/domain/src/action.rs`:
```rust
use easyjob_common::{ActionId, TaskId};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Action {
    pub id: ActionId,
    pub task_id: TaskId,
    pub sequence: u32,
    pub enabled: bool,
    pub kind: ActionKind,
}
```

`crates/domain/src/execution.rs`:
```rust
use chrono::{DateTime, Utc};
use easyjob_common::{ExecutionId, TaskId, TriggerId};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExecutionStatus {
    Queued,
    Running,
    Succeeded,
    Failed,
    TimedOut,
    Cancelled,
    Skipped,
    Interrupted,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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

impl Execution {
    pub fn new(task_id: TaskId, trigger_id: Option<TriggerId>, scheduled_at: Option<DateTime<Utc>>) -> Self {
        Self {
            id: ExecutionId::new(),
            task_id,
            trigger_id,
            status: ExecutionStatus::Queued,
            scheduled_at,
            started_at: Utc::now(),
            finished_at: None,
            duration_ms: None,
            exit_code: None,
            error_message: None,
        }
    }
}
```

`crates/domain/src/task.rs`:
```rust
use crate::action::Action;
use crate::policy::ExecutionPolicy;
use crate::trigger::Trigger;
use chrono::{DateTime, Utc};
use easyjob_common::TaskId;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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

`crates/domain/src/lib.rs`:
```rust
pub mod action;
pub mod execution;
pub mod policy;
pub mod task;
pub mod trigger;

pub use action::{Action, ActionKind};
pub use execution::{Execution, ExecutionStatus};
pub use policy::{ConcurrencyPolicy, ExecutionPolicy, MissedRunPolicy, RetryPolicy};
pub use task::Task;
pub use trigger::{Trigger, TriggerKind};
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p easyjob-domain`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add crates/domain
git commit -m "feat(domain): define pure domain models for task, trigger, action, and execution"
```

---

### Task 3: Trigger Engine & Timezone Evaluator (`crates/scheduler`)

**Files:**
- Create: `crates/scheduler/Cargo.toml`
- Create: `crates/scheduler/src/lib.rs`
- Create: `crates/scheduler/src/evaluator.rs`
- Test: `crates/scheduler/tests/trigger_evaluator_tests.rs`

**Interfaces:**
- Consumes: `easyjob_domain::trigger::TriggerKind`
- Produces:
  - `scheduler::evaluator::{TriggerEvaluator, evaluate_next_occurrence}`

- [ ] **Step 1: Write `crates/scheduler/Cargo.toml`**

```toml
[package]
name = "easyjob-scheduler"
version = "0.1.0"
edition = "2021"

[dependencies]
easyjob-common = { path = "../common" }
easyjob-domain = { path = "../domain" }
chrono.workspace = true
chrono-tz.workspace = true
tokio.workspace = true
tokio-util.workspace = true
tracing.workspace = true
async-trait.workspace = true
```

- [ ] **Step 2: Write failing test in `crates/scheduler/tests/trigger_evaluator_tests.rs`**

```rust
use chrono::{DateTime, NaiveDate, NaiveTime, TimeZone, Utc, Weekday};
use easyjob_domain::trigger::TriggerKind;
use easyjob_scheduler::evaluator::evaluate_next_occurrence;

#[test]
fn test_once_trigger() {
    let fire_at = Utc.with_ymd_and_hms(2026, 10, 1, 10, 0, 0).unwrap();
    let trigger = TriggerKind::Once { fire_at };

    let before = Utc.with_ymd_and_hms(2026, 9, 30, 10, 0, 0).unwrap();
    assert_eq!(evaluate_next_occurrence(&trigger, before), Some(fire_at));

    let after = Utc.with_ymd_and_hms(2026, 10, 1, 10, 0, 1).unwrap();
    assert_eq!(evaluate_next_occurrence(&trigger, after), None);
}

#[test]
fn test_daily_trigger_timezone() {
    let trigger = TriggerKind::Daily {
        time: NaiveTime::from_hms_opt(9, 0, 0).unwrap(),
        timezone: "Asia/Shanghai".to_string(), // UTC+8
    };

    // 2026-09-11 00:00:00 UTC == 2026-09-11 08:00:00 Shanghai
    let current_utc = Utc.with_ymd_and_hms(2026, 9, 11, 0, 0, 0).unwrap();
    let next = evaluate_next_occurrence(&trigger, current_utc).unwrap();

    // 9:00 AM Shanghai on 2026-09-11 is 1:00 AM UTC
    let expected_utc = Utc.with_ymd_and_hms(2026, 9, 11, 1, 0, 0).unwrap();
    assert_eq!(next, expected_utc);
}

#[test]
fn test_interval_trigger() {
    let start_at = Utc.with_ymd_and_hms(2026, 9, 11, 12, 0, 0).unwrap();
    let trigger = TriggerKind::Interval {
        interval_secs: 60,
        start_at: Some(start_at),
    };

    let check_time = Utc.with_ymd_and_hms(2026, 9, 11, 12, 0, 30).unwrap();
    let next = evaluate_next_occurrence(&trigger, check_time).unwrap();
    let expected = Utc.with_ymd_and_hms(2026, 9, 11, 12, 1, 0).unwrap();
    assert_eq!(next, expected);
}
```

- [ ] **Step 3: Run test to verify it fails**

Run: `cargo test -p easyjob-scheduler --test trigger_evaluator_tests`
Expected: FAIL

- [ ] **Step 4: Implement Trigger Evaluator**

`crates/scheduler/src/evaluator.rs`:
```rust
use chrono::{DateTime, Datelike, Duration, NaiveTime, TimeZone, Utc, Weekday};
use chrono_tz::Tz;
use easyjob_domain::trigger::TriggerKind;
use std::str::FromStr;

pub trait TriggerEvaluator {
    fn next_occurrence(&self, after: DateTime<Utc>) -> Option<DateTime<Utc>>;
}

pub fn evaluate_next_occurrence(kind: &TriggerKind, after: DateTime<Utc>) -> Option<DateTime<Utc>> {
    match kind {
        TriggerKind::Once { fire_at } => {
            if *fire_at > after {
                Some(*fire_at)
            } else {
                None
            }
        }
        TriggerKind::Interval { interval_secs, start_at } => {
            if *interval_secs == 0 {
                return None;
            }
            let interval = Duration::seconds(*interval_secs as i64);
            let base = start_at.unwrap_or(after);
            if after < base {
                return Some(base);
            }
            let elapsed = after - base;
            let count = (elapsed.num_seconds() / (*interval_secs as i64)) + 1;
            Some(base + Duration::seconds(count * (*interval_secs as i64)))
        }
        TriggerKind::Daily { time, timezone } => {
            let tz = Tz::from_str(timezone).unwrap_or(chrono_tz::UTC);
            let local_after = after.with_timezone(&tz);
            let mut candidate_date = local_after.date_naive();

            // Try candidate on current date
            if let Some(candidate) = tz.from_local_datetime(&candidate_date.and_time(*time)).single() {
                let candidate_utc = candidate.with_timezone(&Utc);
                if candidate_utc > after {
                    return Some(candidate_utc);
                }
            }

            // Otherwise, next day
            candidate_date = candidate_date.succ_opt()?;
            let next_local = tz.from_local_datetime(&candidate_date.and_time(*time)).single()?;
            Some(next_local.with_timezone(&Utc))
        }
        TriggerKind::Weekly { days_of_week, time, timezone } => {
            if days_of_week.is_empty() {
                return None;
            }
            let tz = Tz::from_str(timezone).unwrap_or(chrono_tz::UTC);
            let local_after = after.with_timezone(&tz);
            let mut candidate_date = local_after.date_naive();

            for _ in 0..14 {
                if days_of_week.contains(&candidate_date.weekday()) {
                    if let Some(candidate) = tz.from_local_datetime(&candidate_date.and_time(*time)).single() {
                        let candidate_utc = candidate.with_timezone(&Utc);
                        if candidate_utc > after {
                            return Some(candidate_utc);
                        }
                    }
                }
                candidate_date = candidate_date.succ_opt()?;
            }
            None
        }
        TriggerKind::AgentStarted => None, // Handled upon agent startup event, not periodic
    }
}
```

`crates/scheduler/src/lib.rs`:
```rust
pub mod evaluator;
pub use evaluator::{evaluate_next_occurrence, TriggerEvaluator};
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p easyjob-scheduler`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add crates/scheduler
git commit -m "feat(scheduler): implement TriggerEvaluator for Once, Daily, Weekly, and Interval"
```

---

### Task 4: Platform Process Management & Tree Kill (`crates/platform`)

**Files:**
- Create: `crates/platform/Cargo.toml`
- Create: `crates/platform/src/lib.rs`
- Create: `crates/platform/src/process.rs`
- Create: `crates/platform/src/unix.rs`
- Create: `crates/platform/src/windows.rs`
- Test: `crates/platform/tests/platform_process_tests.rs`

**Interfaces:**
- Consumes: `easyjob_common::{Error, Result}`
- Produces:
  - `platform::process::{PlatformProcess, CommandBuilder, kill_process_tree}`

- [ ] **Step 1: Write `crates/platform/Cargo.toml`**

```toml
[package]
name = "easyjob-platform"
version = "0.1.0"
edition = "2021"

[dependencies]
easyjob-common = { path = "../common" }
tokio.workspace = true
tracing.workspace = true
async-trait.workspace = true

[target.'cfg(unix)'.dependencies]
libc = "0.2.158"

[target.'cfg(windows)'.dependencies]
windows-sys = { version = "0.59.0", features = ["Win32_System_Threading", "Win32_Foundation", "Win32_Security"] }
```

- [ ] **Step 2: Write failing test in `crates/platform/tests/platform_process_tests.rs`**

```rust
use easyjob_platform::process::CommandBuilder;
use std::time::Duration;

#[tokio::test]
async fn test_spawn_and_capture_echo() {
    let mut cmd = CommandBuilder::new_shell("echo 'hello easyJob'").build();
    let output = cmd.output().await.expect("failed to execute shell command");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("hello easyJob"));
}
```

- [ ] **Step 3: Run test to verify it fails**

Run: `cargo test -p easyjob-platform --test platform_process_tests`
Expected: FAIL

- [ ] **Step 4: Implement platform abstractions**

`crates/platform/src/unix.rs`:
```rust
#[cfg(unix)]
use std::process::Command as StdCommand;
#[cfg(unix)]
use std::os::unix::process::CommandExt;

#[cfg(unix)]
pub fn configure_unix_command(cmd: &mut StdCommand) {
    // Set child as process group leader
    unsafe {
        cmd.pre_exec(|| {
            libc::setpgid(0, 0);
            Ok(())
        });
    }
}

#[cfg(unix)]
pub async fn kill_unix_process_tree(pid: u32) -> easyjob_common::Result<()> {
    unsafe {
        let pgid = pid as i32;
        // Graceful termination
        libc::kill(-pgid, libc::SIGTERM);
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        // Force kill
        libc::kill(-pgid, libc::SIGKILL);
    }
    Ok(())
}
```

`crates/platform/src/windows.rs`:
```rust
#[cfg(windows)]
use std::process::Command as StdCommand;
#[cfg(windows)]
use std::os::windows::process::CommandExt;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x08000000;

#[cfg(windows)]
pub fn configure_windows_command(cmd: &mut StdCommand) {
    cmd.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(windows)]
pub async fn kill_windows_process_tree(pid: u32) -> easyjob_common::Result<()> {
    // taskkill /F /T /PID <pid>
    let _ = tokio::process::Command::new("taskkill")
        .args(["/F", "/T", "/PID", &pid.to_string()])
        .output()
        .await;
    Ok(())
}
```

`crates/platform/src/process.rs`:
```rust
use easyjob_common::Result;
use std::collections::HashMap;
use std::path::PathBuf;
use tokio::process::Command;

pub struct CommandBuilder;

impl CommandBuilder {
    pub fn new_shell(script: &str) -> Command {
        #[cfg(target_os = "windows")]
        {
            let mut cmd = Command::new("cmd.exe");
            cmd.args(["/c", script]);
            crate::windows::configure_windows_command(cmd.as_std_mut());
            cmd
        }
        #[cfg(not(target_os = "windows"))]
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
    }

    pub fn new_program(program: &str, args: &[String]) -> Command {
        let mut cmd = Command::new(program);
        cmd.args(args);
        #[cfg(target_os = "windows")]
        crate::windows::configure_windows_command(cmd.as_std_mut());
        #[cfg(not(target_os = "windows"))]
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
}
```

`crates/platform/src/lib.rs`:
```rust
pub mod process;
#[cfg(unix)]
pub mod unix;
#[cfg(windows)]
pub mod windows;

pub use process::{kill_process_tree, CommandBuilder};
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p easyjob-platform`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add crates/platform
git commit -m "feat(platform): implement cross-platform silent command execution and tree kill"
```

---

### Task 5: Process Runner & Execution Manager (`crates/executor`)

**Files:**
- Create: `crates/executor/Cargo.toml`
- Create: `crates/executor/src/lib.rs`
- Create: `crates/executor/src/runner.rs`
- Create: `crates/executor/src/manager.rs`
- Test: `crates/executor/tests/executor_tests.rs`

**Interfaces:**
- Consumes: `easyjob_domain`, `easyjob_platform`, `easyjob_common`
- Produces:
  - `executor::runner::{ProcessRunner, RunResult}`
  - `executor::manager::{ExecutionManager, ExecutionRequest}`

- [ ] **Step 1: Write `crates/executor/Cargo.toml`**

```toml
[package]
name = "easyjob-executor"
version = "0.1.0"
edition = "2021"

[dependencies]
easyjob-common = { path = "../common" }
easyjob-domain = { path = "../domain" }
easyjob-platform = { path = "../platform" }
tokio.workspace = true
tokio-util.workspace = true
tracing.workspace = true
async-trait.workspace = true
chrono.workspace = true
```

- [ ] **Step 2: Write failing test in `crates/executor/tests/executor_tests.rs`**

```rust
use easyjob_domain::action::{Action, ActionKind};
use easyjob_domain::policy::ConcurrencyPolicy;
use easyjob_common::{ActionId, TaskId};
use easyjob_executor::manager::ExecutionManager;
use easyjob_executor::runner::ProcessRunner;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

#[tokio::test]
async fn test_process_runner_success() {
    let action = Action {
        id: ActionId::new(),
        task_id: TaskId::new(),
        sequence: 1,
        enabled: true,
        kind: ActionKind::ExecuteShell { command: "echo 'running test'".to_string() },
    };

    let cancel = CancellationToken::new();
    let res = ProcessRunner::run_action(&action, None, &Default::default(), Some(5), cancel).await.unwrap();
    assert_eq!(res.exit_code, Some(0));
    assert!(res.stdout.contains("running test"));
}

#[tokio::test]
async fn test_execution_manager_skip_if_running() {
    let manager = ExecutionManager::new(4);
    let task_id = TaskId::new();

    let acquired_first = manager.try_acquire_slot(&task_id, ConcurrencyPolicy::SkipIfRunning).await;
    assert!(acquired_first);

    let acquired_second = manager.try_acquire_slot(&task_id, ConcurrencyPolicy::SkipIfRunning).await;
    assert!(!acquired_second); // Skipped!

    manager.release_slot(&task_id).await;
    let acquired_third = manager.try_acquire_slot(&task_id, ConcurrencyPolicy::SkipIfRunning).await;
    assert!(acquired_third);
}
```

- [ ] **Step 3: Run test to verify it fails**

Run: `cargo test -p easyjob-executor --test executor_tests`
Expected: FAIL

- [ ] **Step 4: Implement Process Runner & Execution Manager**

`crates/executor/src/runner.rs`:
```rust
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

const MAX_OUTPUT_BYTES: usize = 2 * 1024 * 1024; // 2MB

#[derive(Debug, Clone)]
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
        let mut cmd = match &action.kind {
            ActionKind::ExecuteProgram { program, args } => {
                CommandBuilder::new_program(program, args)
            }
            ActionKind::ExecuteShell { command } => {
                CommandBuilder::new_shell(command)
            }
            ActionKind::ExecuteCmd { command } => {
                let mut c = tokio::process::Command::new("cmd.exe");
                c.args(["/c", command]);
                c
            }
            ActionKind::ExecutePowerShell { script, no_profile } => {
                let mut c = tokio::process::Command::new("powershell.exe");
                if *no_profile {
                    c.arg("-NoProfile");
                }
                c.args(["-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", script]);
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

        let mut stdout_pipe = child.stdout.take();
        let mut stderr_pipe = child.stderr.take();

        let stdout_handle = tokio::spawn(async move {
            let mut buf = Vec::new();
            if let Some(mut pipe) = stdout_pipe {
                let mut chunk = [0u8; 4096];
                while let Ok(n) = pipe.read(&mut chunk).await {
                    if n == 0 { break; }
                    if buf.len() + n <= MAX_OUTPUT_BYTES {
                        buf.extend_from_slice(&chunk[..n]);
                    } else if buf.len() < MAX_OUTPUT_BYTES {
                        let rem = MAX_OUTPUT_BYTES - buf.len();
                        buf.extend_from_slice(&chunk[..rem]);
                        buf.extend_from_slice(b"\n... [output truncated]");
                    }
                }
            }
            String::from_utf8_lossy(&buf).to_string()
        });

        let stderr_handle = tokio::spawn(async move {
            let mut buf = Vec::new();
            if let Some(mut pipe) = stderr_pipe {
                let mut chunk = [0u8; 4096];
                while let Ok(n) = pipe.read(&mut chunk).await {
                    if n == 0 { break; }
                    if buf.len() + n <= MAX_OUTPUT_BYTES {
                        buf.extend_from_slice(&chunk[..n]);
                    } else if buf.len() < MAX_OUTPUT_BYTES {
                        let rem = MAX_OUTPUT_BYTES - buf.len();
                        buf.extend_from_slice(&chunk[..rem]);
                        buf.extend_from_slice(b"\n... [output truncated]");
                    }
                }
            }
            String::from_utf8_lossy(&buf).to_string()
        });

        let wait_fut = async { child.wait().await };
        let timeout_duration = Duration::from_secs(timeout_secs.unwrap_or(86400));

        tokio::select! {
            _ = cancel.cancelled() => {
                if let Some(p) = pid { let _ = kill_process_tree(p).await; }
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
                        if let Some(p) = pid { let _ = kill_process_tree(p).await; }
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
```

`crates/executor/src/manager.rs`:
```rust
use easyjob_common::TaskId;
use easyjob_domain::policy::ConcurrencyPolicy;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{Mutex, Semaphore};

pub struct ExecutionManager {
    global_limiter: Arc<Semaphore>,
    task_running_counts: Arc<Mutex<HashMap<TaskId, usize>>>,
}

impl ExecutionManager {
    pub fn new(max_concurrent: usize) -> Self {
        Self {
            global_limiter: Arc::new(Semaphore::new(max_concurrent)),
            task_running_counts: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub async fn try_acquire_slot(&self, task_id: &TaskId, policy: ConcurrencyPolicy) -> bool {
        let mut counts = self.task_running_counts.lock().await;
        let count = counts.entry(*task_id).or_insert(0);

        match policy {
            ConcurrencyPolicy::AllowParallel => {
                *count += 1;
                true
            }
            ConcurrencyPolicy::SkipIfRunning => {
                if *count > 0 {
                    false
                } else {
                    *count += 1;
                    true
                }
            }
            ConcurrencyPolicy::QueueOne => {
                if *count >= 2 {
                    false
                } else {
                    *count += 1;
                    true
                }
            }
        }
    }

    pub async fn release_slot(&self, task_id: &TaskId) {
        let mut counts = self.task_running_counts.lock().await;
        if let Some(count) = counts.get_mut(task_id) {
            if *count > 0 {
                *count -= 1;
            }
        }
    }

    pub fn global_semaphore(&self) -> Arc<Semaphore> {
        self.global_limiter.clone()
    }
}
```

`crates/executor/src/lib.rs`:
```rust
pub mod manager;
pub mod runner;

pub use manager::ExecutionManager;
pub use runner::{ProcessRunner, RunResult};
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p easyjob-executor`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add crates/executor
git commit -m "feat(executor): implement ProcessRunner with output capture, timeout, and ExecutionManager concurrency control"
```

---

### Task 6: SQLite Schema Migrations & Connection Pool (`crates/persistence`)

**Files:**
- Create: `crates/persistence/Cargo.toml`
- Create: `migrations/20260911000000_init_schema.sql`
- Create: `crates/persistence/src/lib.rs`
- Create: `crates/persistence/src/db.rs`
- Test: `crates/persistence/tests/migration_tests.rs`

**Interfaces:**
- Consumes: `easyjob_common`
- Produces:
  - `persistence::db::{init_pool, run_migrations, DbPool}`

- [ ] **Step 1: Write `crates/persistence/Cargo.toml`**

```toml
[package]
name = "easyjob-persistence"
version = "0.1.0"
edition = "2021"

[dependencies]
easyjob-common = { path = "../common" }
easyjob-domain = { path = "../domain" }
sqlx.workspace = true
tokio.workspace = true
chrono.workspace = true
serde.workspace = true
serde_json.workspace = true
uuid.workspace = true
async-trait.workspace = true
tracing.workspace = true
```

- [ ] **Step 2: Write SQLite Migration in `migrations/20260911000000_init_schema.sql`**

```sql
CREATE TABLE IF NOT EXISTS tasks (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    description TEXT,
    enabled INTEGER NOT NULL,
    execution_policy_json TEXT NOT NULL,
    working_directory TEXT,
    environment_json TEXT NOT NULL,
    version INTEGER NOT NULL DEFAULT 1,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS triggers (
    id TEXT PRIMARY KEY,
    task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    kind TEXT NOT NULL,
    config_json TEXT NOT NULL,
    enabled INTEGER NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_triggers_task_id ON triggers(task_id);

CREATE TABLE IF NOT EXISTS actions (
    id TEXT PRIMARY KEY,
    task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    sequence INTEGER NOT NULL,
    kind TEXT NOT NULL,
    config_json TEXT NOT NULL,
    enabled INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_actions_task_id ON actions(task_id);

CREATE TABLE IF NOT EXISTS task_runs (
    id TEXT PRIMARY KEY,
    task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    trigger_id TEXT,
    status TEXT NOT NULL,
    scheduled_at TEXT,
    started_at TEXT NOT NULL,
    finished_at TEXT,
    duration_ms INTEGER,
    exit_code INTEGER,
    error_message TEXT
);

CREATE INDEX IF NOT EXISTS idx_runs_task_id ON task_runs(task_id);

CREATE TABLE IF NOT EXISTS run_outputs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    run_id TEXT NOT NULL REFERENCES task_runs(id) ON DELETE CASCADE,
    stream TEXT NOT NULL,
    content TEXT NOT NULL,
    created_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_outputs_run_id ON run_outputs(run_id);

CREATE TABLE IF NOT EXISTS agent_state (
    key TEXT PRIMARY KEY,
    value_json TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS settings (
    key TEXT PRIMARY KEY,
    value_json TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
```

- [ ] **Step 3: Write failing test in `crates/persistence/tests/migration_tests.rs`**

```rust
use easyjob_persistence::db::init_pool;

#[tokio::test]
async fn test_in_memory_db_migrations() {
    let pool = init_pool("sqlite::memory:").await.unwrap();
    let row: (i64,) = sqlx::query_as("SELECT count(*) FROM tasks")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(row.0, 0);
}
```

- [ ] **Step 4: Implement DB Connection & Migration**

`crates/persistence/src/db.rs`:
```rust
use easyjob_common::{Error, Result};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::{Pool, Sqlite};
use std::str::FromStr;

pub type DbPool = Pool<Sqlite>;

pub async fn init_pool(url: &str) -> Result<DbPool> {
    let options = SqliteConnectOptions::from_str(url)
        .map_err(|e| Error::Database(e.to_string()))?
        .create_if_missing(true)
        .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
        .foreign_keys(true)
        .busy_timeout(std::time::Duration::from_secs(5));

    let pool = SqlitePoolOptions::new()
        .max_connections(10)
        .connect_with(options)
        .await
        .map_err(|e| Error::Database(e.to_string()))?;

    run_migrations(&pool).await?;
    Ok(pool)
}

pub async fn run_migrations(pool: &DbPool) -> Result<()> {
    sqlx::migrate!("../../migrations")
        .run(pool)
        .await
        .map_err(|e| Error::Database(e.to_string()))?;
    Ok(())
}
```

`crates/persistence/src/lib.rs`:
```rust
pub mod db;
pub use db::{init_pool, run_migrations, DbPool};
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p easyjob-persistence --test migration_tests`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add migrations crates/persistence
git commit -m "feat(persistence): add SQLite schema migrations with WAL and connection pool"
```

---

### Task 7: Repositories & Crash Recovery (`crates/persistence`)

**Files:**
- Create: `crates/persistence/src/task_repo.rs`
- Create: `crates/persistence/src/execution_repo.rs`
- Create: `crates/persistence/src/recovery.rs`
- Modify: `crates/persistence/src/lib.rs`
- Test: `crates/persistence/tests/repository_tests.rs`

**Interfaces:**
- Consumes: `easyjob_domain`, `easyjob_common`, `persistence::db::DbPool`
- Produces:
  - `persistence::task_repo::{TaskRepository, SqliteTaskRepository}`
  - `persistence::execution_repo::{ExecutionRepository, SqliteExecutionRepository}`
  - `persistence::recovery::recover_dangling_executions`

- [ ] **Step 1: Write failing test in `crates/persistence/tests/repository_tests.rs`**

```rust
use easyjob_common::{ActionId, ExecutionId, TaskId, TriggerId};
use easyjob_domain::action::{Action, ActionKind};
use easyjob_domain::execution::{Execution, ExecutionStatus};
use easyjob_domain::policy::ExecutionPolicy;
use easyjob_domain::task::Task;
use easyjob_domain::trigger::{Trigger, TriggerKind};
use easyjob_persistence::db::init_pool;
use easyjob_persistence::execution_repo::{ExecutionRepository, SqliteExecutionRepository};
use easyjob_persistence::recovery::recover_dangling_executions;
use easyjob_persistence::task_repo::{SqliteTaskRepository, TaskRepository};
use chrono::Utc;
use std::collections::HashMap;

#[tokio::test]
async fn test_task_save_load_and_recovery() {
    let pool = init_pool("sqlite::memory:").await.unwrap();
    let task_repo = SqliteTaskRepository::new(pool.clone());
    let exec_repo = SqliteExecutionRepository::new(pool.clone());

    let task_id = TaskId::new();
    let task = Task {
        id: task_id,
        name: "Repo Test Task".to_string(),
        description: None,
        enabled: true,
        triggers: vec![Trigger {
            id: TriggerId::new(),
            task_id,
            enabled: true,
            kind: TriggerKind::AgentStarted,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }],
        actions: vec![Action {
            id: ActionId::new(),
            task_id,
            sequence: 1,
            enabled: true,
            kind: ActionKind::ExecuteShell { command: "true".to_string() },
        }],
        execution_policy: ExecutionPolicy::default(),
        working_directory: None,
        environment: HashMap::new(),
        version: 1,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };

    task_repo.save(&task).await.unwrap();
    let loaded = task_repo.find_by_id(&task_id).await.unwrap().expect("task found");
    assert_eq!(loaded.name, "Repo Test Task");
    assert_eq!(loaded.triggers.len(), 1);
    assert_eq!(loaded.actions.len(), 1);

    // Test execution dangling recovery
    let mut dangling_exec = Execution::new(task_id, None, None);
    dangling_exec.status = ExecutionStatus::Running;
    exec_repo.create_run(&dangling_exec).await.unwrap();

    let recovered = recover_dangling_executions(&pool).await.unwrap();
    assert_eq!(recovered, 1);

    let updated_exec = exec_repo.find_run_by_id(&dangling_exec.id).await.unwrap().unwrap();
    assert_eq!(updated_exec.status, ExecutionStatus::Interrupted);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p easyjob-persistence --test repository_tests`
Expected: FAIL

- [ ] **Step 3: Implement Repositories and Recovery**

`crates/persistence/src/task_repo.rs`:
```rust
use async_trait::async_trait;
use chrono::Utc;
use easyjob_common::{Error, Result, TaskId};
use easyjob_domain::action::Action;
use easyjob_domain::task::Task;
use easyjob_domain::trigger::Trigger;
use sqlx::{Row, SqlitePool};
use std::path::PathBuf;

#[async_trait]
pub trait TaskRepository: Send + Sync {
    async fn find_all_enabled(&self) -> Result<Vec<Task>>;
    async fn find_by_id(&self, id: &TaskId) -> Result<Option<Task>>;
    async fn save(&self, task: &Task) -> Result<()>;
    async fn delete(&self, id: &TaskId) -> Result<()>;
}

pub struct SqliteTaskRepository {
    pool: SqlitePool,
}

impl SqliteTaskRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl TaskRepository for SqliteTaskRepository {
    async fn find_all_enabled(&self) -> Result<Vec<Task>> {
        let rows = sqlx::query("SELECT id FROM tasks WHERE enabled = 1")
            .fetch_all(&self.pool)
            .await
            .map_err(|e| Error::Database(e.to_string()))?;

        let mut tasks = Vec::new();
        for row in rows {
            let id_str: String = row.get("id");
            if let Ok(id) = TaskId::parse(&id_str) {
                if let Some(task) = self.find_by_id(&id).await? {
                    tasks.push(task);
                }
            }
        }
        Ok(tasks)
    }

    async fn find_by_id(&self, id: &TaskId) -> Result<Option<Task>> {
        let task_row = sqlx::query(
            "SELECT id, name, description, enabled, execution_policy_json, working_directory, environment_json, version, created_at, updated_at
             FROM tasks WHERE id = ?"
        )
        .bind(id.to_string())
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| Error::Database(e.to_string()))?;

        let row = match task_row {
            Some(r) => r,
            None => return Ok(None),
        };

        let trigger_rows = sqlx::query("SELECT config_json FROM triggers WHERE task_id = ? AND enabled = 1")
            .bind(id.to_string())
            .fetch_all(&self.pool)
            .await
            .map_err(|e| Error::Database(e.to_string()))?;

        let mut triggers = Vec::new();
        for tr in trigger_rows {
            let json: String = tr.get("config_json");
            if let Ok(t) = serde_json::from_str::<Trigger>(&json) {
                triggers.push(t);
            }
        }

        let action_rows = sqlx::query("SELECT config_json FROM actions WHERE task_id = ? AND enabled = 1 ORDER BY sequence ASC")
            .bind(id.to_string())
            .fetch_all(&self.pool)
            .await
            .map_err(|e| Error::Database(e.to_string()))?;

        let mut actions = Vec::new();
        for ar in action_rows {
            let json: String = ar.get("config_json");
            if let Ok(a) = serde_json::from_str::<Action>(&json) {
                actions.push(a);
            }
        }

        let policy_json: String = row.get("execution_policy_json");
        let env_json: String = row.get("environment_json");
        let wd: Option<String> = row.get("working_directory");

        let task = Task {
            id: *id,
            name: row.get("name"),
            description: row.get("description"),
            enabled: row.get::<i64, _>("enabled") == 1,
            triggers,
            actions,
            execution_policy: serde_json::from_str(&policy_json)?,
            working_directory: wd.map(PathBuf::from),
            environment: serde_json::from_str(&env_json)?,
            version: row.get("version"),
            created_at: row.get::<String, _>("created_at").parse().unwrap_or_else(|_| Utc::now()),
            updated_at: row.get::<String, _>("updated_at").parse().unwrap_or_else(|_| Utc::now()),
        };

        Ok(Some(task))
    }

    async fn save(&self, task: &Task) -> Result<()> {
        let mut tx = self.pool.begin().await.map_err(|e| Error::Database(e.to_string()))?;

        let policy_json = serde_json::to_string(&task.execution_policy)?;
        let env_json = serde_json::to_string(&task.environment)?;
        let wd_str = task.working_directory.as_ref().map(|p| p.to_string_lossy().to_string());

        sqlx::query(
            "INSERT INTO tasks (id, name, description, enabled, execution_policy_json, working_directory, environment_json, version, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(id) DO UPDATE SET
                name = excluded.name,
                description = excluded.description,
                enabled = excluded.enabled,
                execution_policy_json = excluded.execution_policy_json,
                working_directory = excluded.working_directory,
                environment_json = excluded.environment_json,
                version = excluded.version + 1,
                updated_at = excluded.updated_at"
        )
        .bind(task.id.to_string())
        .bind(&task.name)
        .bind(&task.description)
        .bind(if task.enabled { 1 } else { 0 })
        .bind(policy_json)
        .bind(wd_str)
        .bind(env_json)
        .bind(task.version)
        .bind(task.created_at.to_rfc3339())
        .bind(task.updated_at.to_rfc3339())
        .execute(&mut *tx)
        .await
        .map_err(|e| Error::Database(e.to_string()))?;

        sqlx::query("DELETE FROM triggers WHERE task_id = ?")
            .bind(task.id.to_string())
            .execute(&mut *tx)
            .await
            .map_err(|e| Error::Database(e.to_string()))?;

        for trigger in &task.triggers {
            let kind_str = format!("{:?}", trigger.kind);
            let config_json = serde_json::to_string(trigger)?;
            sqlx::query(
                "INSERT INTO triggers (id, task_id, kind, config_json, enabled, created_at, updated_at)
                 VALUES (?, ?, ?, ?, ?, ?, ?)"
            )
            .bind(trigger.id.to_string())
            .bind(task.id.to_string())
            .bind(kind_str)
            .bind(config_json)
            .bind(if trigger.enabled { 1 } else { 0 })
            .bind(trigger.created_at.to_rfc3339())
            .bind(trigger.updated_at.to_rfc3339())
            .execute(&mut *tx)
            .await
            .map_err(|e| Error::Database(e.to_string()))?;
        }

        sqlx::query("DELETE FROM actions WHERE task_id = ?")
            .bind(task.id.to_string())
            .execute(&mut *tx)
            .await
            .map_err(|e| Error::Database(e.to_string()))?;

        for action in &task.actions {
            let kind_str = format!("{:?}", action.kind);
            let config_json = serde_json::to_string(action)?;
            sqlx::query(
                "INSERT INTO actions (id, task_id, sequence, kind, config_json, enabled)
                 VALUES (?, ?, ?, ?, ?, ?)"
            )
            .bind(action.id.to_string())
            .bind(task.id.to_string())
            .bind(action.sequence)
            .bind(kind_str)
            .bind(config_json)
            .bind(if action.enabled { 1 } else { 0 })
            .execute(&mut *tx)
            .await
            .map_err(|e| Error::Database(e.to_string()))?;
        }

        tx.commit().await.map_err(|e| Error::Database(e.to_string()))?;
        Ok(())
    }

    async fn delete(&self, id: &TaskId) -> Result<()> {
        sqlx::query("DELETE FROM tasks WHERE id = ?")
            .bind(id.to_string())
            .execute(&self.pool)
            .await
            .map_err(|e| Error::Database(e.to_string()))?;
        Ok(())
    }
}
```

`crates/persistence/src/execution_repo.rs`:
```rust
use async_trait::async_trait;
use chrono::Utc;
use easyjob_common::{Error, ExecutionId, Result, TaskId, TriggerId};
use easyjob_domain::execution::{Execution, ExecutionStatus};
use sqlx::{Row, SqlitePool};

#[async_trait]
pub trait ExecutionRepository: Send + Sync {
    async fn create_run(&self, run: &Execution) -> Result<()>;
    async fn update_run(&self, run: &Execution) -> Result<()>;
    async fn find_run_by_id(&self, id: &ExecutionId) -> Result<Option<Execution>>;
    async fn append_output(&self, run_id: &ExecutionId, stream: &str, content: &str) -> Result<()>;
}

pub struct SqliteExecutionRepository {
    pool: SqlitePool,
}

impl SqliteExecutionRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl ExecutionRepository for SqliteExecutionRepository {
    async fn create_run(&self, run: &Execution) -> Result<()> {
        sqlx::query(
            "INSERT INTO task_runs (id, task_id, trigger_id, status, scheduled_at, started_at, finished_at, duration_ms, exit_code, error_message)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"
        )
        .bind(run.id.to_string())
        .bind(run.task_id.to_string())
        .bind(run.trigger_id.map(|t| t.to_string()))
        .bind(serde_json::to_string(&run.status)?)
        .bind(run.scheduled_at.map(|s| s.to_rfc3339()))
        .bind(run.started_at.to_rfc3339())
        .bind(run.finished_at.map(|f| f.to_rfc3339()))
        .bind(run.duration_ms.map(|d| d as i64))
        .bind(run.exit_code)
        .bind(&run.error_message)
        .execute(&self.pool)
        .await
        .map_err(|e| Error::Database(e.to_string()))?;
        Ok(())
    }

    async fn update_run(&self, run: &Execution) -> Result<()> {
        sqlx::query(
            "UPDATE task_runs SET
                status = ?,
                finished_at = ?,
                duration_ms = ?,
                exit_code = ?,
                error_message = ?
             WHERE id = ?"
        )
        .bind(serde_json::to_string(&run.status)?)
        .bind(run.finished_at.map(|f| f.to_rfc3339()))
        .bind(run.duration_ms.map(|d| d as i64))
        .bind(run.exit_code)
        .bind(&run.error_message)
        .bind(run.id.to_string())
        .execute(&self.pool)
        .await
        .map_err(|e| Error::Database(e.to_string()))?;
        Ok(())
    }

    async fn find_run_by_id(&self, id: &ExecutionId) -> Result<Option<Execution>> {
        let row = sqlx::query("SELECT * FROM task_runs WHERE id = ?")
            .bind(id.to_string())
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| Error::Database(e.to_string()))?;

        let r = match row {
            Some(row) => row,
            None => return Ok(None),
        };

        let status_json: String = r.get("status");
        let status: ExecutionStatus = serde_json::from_str(&status_json)?;
        let task_id_str: String = r.get("task_id");
        let trigger_id_str: Option<String> = r.get("trigger_id");
        let sched_str: Option<String> = r.get("scheduled_at");
        let start_str: String = r.get("started_at");
        let finish_str: Option<String> = r.get("finished_at");
        let dur: Option<i64> = r.get("duration_ms");

        Ok(Some(Execution {
            id: *id,
            task_id: TaskId::parse(&task_id_str).unwrap(),
            trigger_id: trigger_id_str.and_then(|s| TriggerId::parse(&s).ok()),
            status,
            scheduled_at: sched_str.and_then(|s| s.parse().ok()),
            started_at: start_str.parse().unwrap_or_else(|_| Utc::now()),
            finished_at: finish_str.and_then(|s| s.parse().ok()),
            duration_ms: dur.map(|d| d as u64),
            exit_code: r.get("exit_code"),
            error_message: r.get("error_message"),
        }))
    }

    async fn append_output(&self, run_id: &ExecutionId, stream: &str, content: &str) -> Result<()> {
        sqlx::query(
            "INSERT INTO run_outputs (run_id, stream, content, created_at) VALUES (?, ?, ?, ?)"
        )
        .bind(run_id.to_string())
        .bind(stream)
        .bind(content)
        .bind(Utc::now().to_rfc3339())
        .execute(&self.pool)
        .await
        .map_err(|e| Error::Database(e.to_string()))?;
        Ok(())
    }
}
```

`crates/persistence/src/recovery.rs`:
```rust
use easyjob_common::{Error, Result};
use easyjob_domain::execution::ExecutionStatus;
use sqlx::SqlitePool;

pub async fn recover_dangling_executions(pool: &SqlitePool) -> Result<u64> {
    let running_status = serde_json::to_string(&ExecutionStatus::Running)?;
    let interrupted_status = serde_json::to_string(&ExecutionStatus::Interrupted)?;

    let res = sqlx::query(
        "UPDATE task_runs SET status = ?, error_message = 'Agent restarted unexpectedly'
         WHERE status = ?"
    )
    .bind(interrupted_status)
    .bind(running_status)
    .execute(pool)
    .await
    .map_err(|e| Error::Database(e.to_string()))?;

    Ok(res.rows_affected())
}
```

Update `crates/persistence/src/lib.rs`:
```rust
pub mod db;
pub mod execution_repo;
pub mod recovery;
pub mod task_repo;

pub use db::{init_pool, run_migrations, DbPool};
pub use execution_repo::{ExecutionRepository, SqliteExecutionRepository};
pub use recovery::recover_dangling_executions;
pub use task_repo::{SqliteTaskRepository, TaskRepository};
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p easyjob-persistence`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/persistence
git commit -m "feat(persistence): implement SqliteTaskRepository, SqliteExecutionRepository, and crash recovery"
```

---

### Task 8: Min-Heap Priority Queue & Scheduler Loop (`crates/scheduler`)

**Files:**
- Create: `crates/scheduler/src/queue.rs`
- Create: `crates/scheduler/src/scheduler.rs`
- Modify: `crates/scheduler/src/lib.rs`
- Test: `crates/scheduler/tests/scheduler_loop_tests.rs`

**Interfaces:**
- Consumes: `easyjob_domain`, `easyjob_common`, `easyjob_persistence`
- Produces:
  - `scheduler::queue::{ScheduledItem, ScheduleQueue}`
  - `scheduler::scheduler::{Scheduler, SchedulerCommand}`

- [ ] **Step 1: Write failing test in `crates/scheduler/tests/scheduler_loop_tests.rs`**

```rust
use chrono::{Duration, Utc};
use easyjob_common::{TaskId, TriggerId};
use easyjob_scheduler::queue::{ScheduleQueue, ScheduledItem};

#[test]
fn test_schedule_queue_ordering_and_invalidation() {
    let mut queue = ScheduleQueue::new();
    let task1 = TaskId::new();
    let task2 = TaskId::new();

    let now = Utc::now();
    let item1 = ScheduledItem {
        task_id: task1,
        trigger_id: TriggerId::new(),
        next_fire_at: now + Duration::seconds(10),
        generation: 1,
    };
    let item2 = ScheduledItem {
        task_id: task2,
        trigger_id: TriggerId::new(),
        next_fire_at: now + Duration::seconds(5),
        generation: 1,
    };

    queue.push(item1);
    queue.push(item2);

    // Earliest must be popped first
    let popped = queue.pop().expect("item in queue");
    assert_eq!(popped.task_id, task2);

    // Invalidate task1
    queue.bump_generation(&task1);
    assert!(!queue.is_valid(&popped));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p easyjob-scheduler --test scheduler_loop_tests`
Expected: FAIL

- [ ] **Step 3: Implement Queue & Scheduler Loop**

`crates/scheduler/src/queue.rs`:
```rust
use chrono::{DateTime, Utc};
use easyjob_common::{TaskId, TriggerId};
use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap};

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ScheduledItem {
    pub task_id: TaskId,
    pub trigger_id: TriggerId,
    pub next_fire_at: DateTime<Utc>,
    pub generation: u64,
}

impl Ord for ScheduledItem {
    fn cmp(&self, other: &Self) -> Ordering {
        // Reverse for MinHeap (earliest time first)
        other.next_fire_at.cmp(&self.next_fire_at)
            .then_with(|| self.task_id.cmp(&other.task_id))
    }
}

impl PartialOrd for ScheduledItem {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[derive(Default)]
pub struct ScheduleQueue {
    heap: BinaryHeap<ScheduledItem>,
    generations: HashMap<TaskId, u64>,
}

impl ScheduleQueue {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, item: ScheduledItem) {
        let current_gen = self.generations.entry(item.task_id).or_insert(1);
        if item.generation == *current_gen {
            self.heap.push(item);
        }
    }

    pub fn pop(&mut self) -> Option<ScheduledItem> {
        while let Some(item) = self.heap.pop() {
            if self.is_valid(&item) {
                return Some(item);
            }
        }
        None
    }

    pub fn peek(&self) -> Option<&ScheduledItem> {
        self.heap.peek()
    }

    pub fn bump_generation(&mut self, task_id: &TaskId) -> u64 {
        let gen = self.generations.entry(*task_id).or_insert(1);
        *gen += 1;
        *gen
    }

    pub fn current_generation(&self, task_id: &TaskId) -> u64 {
        self.generations.get(task_id).copied().unwrap_or(1)
    }

    pub fn is_valid(&self, item: &ScheduledItem) -> bool {
        self.generations.get(&item.task_id).copied().unwrap_or(1) == item.generation
    }

    pub fn clear(&mut self) {
        self.heap.clear();
    }
}
```

`crates/scheduler/src/scheduler.rs`:
```rust
use crate::evaluator::evaluate_next_occurrence;
use crate::queue::{ScheduleQueue, ScheduledItem};
use chrono::{DateTime, Utc};
use easyjob_common::{TaskId, TriggerId};
use easyjob_domain::task::Task;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc::{self, Receiver, Sender};
use tokio::sync::Mutex;
use tracing::{info, warn};

pub enum SchedulerCommand {
    AddTask(Task),
    RemoveTask(TaskId),
    TriggerNow(TaskId),
    Shutdown,
}

#[derive(Debug, Clone)]
pub struct TriggerEvent {
    pub task_id: TaskId,
    pub trigger_id: Option<TriggerId>,
    pub scheduled_at: DateTime<Utc>,
}

pub struct Scheduler {
    queue: Arc<Mutex<ScheduleQueue>>,
    cmd_tx: Sender<SchedulerCommand>,
    event_tx: Sender<TriggerEvent>,
}

impl Scheduler {
    pub fn new(event_tx: Sender<TriggerEvent>) -> (Self, Receiver<SchedulerCommand>) {
        let (cmd_tx, cmd_rx) = mpsc::channel(100);
        let scheduler = Self {
            queue: Arc::new(Mutex::new(ScheduleQueue::new())),
            cmd_tx,
            event_tx,
        };
        (scheduler, cmd_rx)
    }

    pub fn sender(&self) -> Sender<SchedulerCommand> {
        self.cmd_tx.clone()
    }

    pub async fn run(
        queue: Arc<Mutex<ScheduleQueue>>,
        mut cmd_rx: Receiver<SchedulerCommand>,
        event_tx: Sender<TriggerEvent>,
    ) {
        let mut last_wall_clock = Utc::now();

        loop {
            let next_deadline = {
                let mut q = queue.lock().await;
                q.pop()
            };

            let sleep_duration = match &next_deadline {
                Some(item) => {
                    let now = Utc::now();
                    if item.next_fire_at <= now {
                        Duration::ZERO
                    } else {
                        (item.next_fire_at - now).to_std().unwrap_or(Duration::ZERO)
                    }
                }
                None => Duration::from_secs(3600), // Idle wait if queue empty
            };

            tokio::select! {
                cmd = cmd_rx.recv() => {
                    match cmd {
                        Some(SchedulerCommand::Shutdown) | None => {
                            info!("Scheduler loop stopping");
                            break;
                        }
                        Some(SchedulerCommand::AddTask(task)) => {
                            let mut q = queue.lock().await;
                            let gen = q.bump_generation(&task.id);
                            for tr in &task.triggers {
                                if tr.enabled {
                                    if let Some(next) = evaluate_next_occurrence(&tr.kind, Utc::now()) {
                                        q.push(ScheduledItem {
                                            task_id: task.id,
                                            trigger_id: tr.id,
                                            next_fire_at: next,
                                            generation: gen,
                                        });
                                    }
                                }
                            }
                        }
                        Some(SchedulerCommand::RemoveTask(id)) => {
                            let mut q = queue.lock().await;
                            q.bump_generation(&id);
                        }
                        Some(SchedulerCommand::TriggerNow(id)) => {
                            let _ = event_tx.send(TriggerEvent {
                                task_id: id,
                                trigger_id: None,
                                scheduled_at: Utc::now(),
                            }).await;
                        }
                    }
                }
                _ = tokio::time::sleep(sleep_duration), if next_deadline.is_some() => {
                    if let Some(item) = next_deadline {
                        let _ = event_tx.send(TriggerEvent {
                            task_id: item.task_id,
                            trigger_id: Some(item.trigger_id),
                            scheduled_at: item.next_fire_at,
                        }).await;
                    }
                }
                _ = tokio::time::sleep(Duration::from_secs(10)) => {
                    // Wall clock jump detector
                    let now = Utc::now();
                    let diff = (now - last_wall_clock).num_seconds();
                    last_wall_clock = now;
                    if diff.abs() > 30 {
                        warn!("System clock jump detected ({}s); scheduler queue recheck warranted", diff);
                    }
                }
            }
        }
    }
}
```

Update `crates/scheduler/src/lib.rs`:
```rust
pub mod evaluator;
pub mod queue;
pub mod scheduler;

pub use evaluator::{evaluate_next_occurrence, TriggerEvaluator};
pub use queue::{ScheduleQueue, ScheduledItem};
pub use scheduler::{Scheduler, SchedulerCommand, TriggerEvent};
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p easyjob-scheduler`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/scheduler
git commit -m "feat(scheduler): implement ScheduleQueue Min-Heap and asynchronous Scheduler loop"
```

---

### Task 9: Phase 1 Engine End-to-End Integration Test

**Files:**
- Create: `tests/engine_integration_test.rs`

- [ ] **Step 1: Write integration test connecting persistence, scheduler, and executor**

`tests/engine_integration_test.rs`:
```rust
use chrono::Utc;
use easyjob_common::{ActionId, TaskId, TriggerId};
use easyjob_domain::action::{Action, ActionKind};
use easyjob_domain::execution::ExecutionStatus;
use easyjob_domain::policy::{ConcurrencyPolicy, ExecutionPolicy, MissedRunPolicy, RetryPolicy};
use easyjob_domain::task::Task;
use easyjob_domain::trigger::{Trigger, TriggerKind};
use easyjob_executor::manager::ExecutionManager;
use easyjob_executor::runner::ProcessRunner;
use easyjob_persistence::db::init_pool;
use easyjob_persistence::execution_repo::{ExecutionRepository, SqliteExecutionRepository};
use easyjob_persistence::task_repo::{SqliteTaskRepository, TaskRepository};
use easyjob_scheduler::scheduler::{Scheduler, SchedulerCommand, TriggerEvent};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

#[tokio::test]
async fn test_end_to_end_task_trigger_and_execute() {
    // 1. Initialize DB
    let pool = init_pool("sqlite::memory:").await.unwrap();
    let task_repo = SqliteTaskRepository::new(pool.clone());
    let exec_repo = SqliteExecutionRepository::new(pool.clone());

    // 2. Create Task
    let task_id = TaskId::new();
    let action_id = ActionId::new();
    let trigger_id = TriggerId::new();

    let task = Task {
        id: task_id,
        name: "E2E Test Task".to_string(),
        description: Some("Integration test runner".to_string()),
        enabled: true,
        triggers: vec![Trigger {
            id: trigger_id,
            task_id,
            enabled: true,
            kind: TriggerKind::Interval { interval_secs: 1, start_at: None },
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }],
        actions: vec![Action {
            id: action_id,
            task_id,
            sequence: 1,
            enabled: true,
            kind: ActionKind::ExecuteShell { command: "echo 'E2E success'".to_string() },
        }],
        execution_policy: ExecutionPolicy {
            concurrency_policy: ConcurrencyPolicy::AllowParallel,
            missed_run_policy: MissedRunPolicy::RunOnce,
            retry_policy: RetryPolicy::default(),
            timeout_secs: Some(5),
        },
        working_directory: None,
        environment: HashMap::new(),
        version: 1,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };

    task_repo.save(&task).await.unwrap();

    // 3. Setup Scheduler & Execution Manager
    let (event_tx, mut event_rx) = mpsc::channel(10);
    let (scheduler, mut cmd_rx) = Scheduler::new(event_tx);
    let exec_manager = Arc::new(ExecutionManager::new(4));

    // Manually trigger task
    scheduler.sender().send(SchedulerCommand::TriggerNow(task_id)).await.unwrap();

    // 4. Handle Trigger Event & Execute
    let event: TriggerEvent = event_rx.recv().await.unwrap();
    assert_eq!(event.task_id, task_id);

    let loaded_task = task_repo.find_by_id(&event.task_id).await.unwrap().unwrap();
    let action = &loaded_task.actions[0];
    let cancel = CancellationToken::new();

    let res = ProcessRunner::run_action(
        action,
        loaded_task.working_directory.as_ref(),
        &loaded_task.environment,
        loaded_task.execution_policy.timeout_secs,
        cancel,
    ).await.unwrap();

    assert_eq!(res.status, ExecutionStatus::Succeeded);
    assert!(res.stdout.contains("E2E success"));
}
```

- [ ] **Step 2: Run all workspace tests**

Run: `cargo test --all`
Expected: ALL PASS

- [ ] **Step 3: Commit**

```bash
git add tests/engine_integration_test.rs
git commit -m "test: add end-to-end integration test for Phase 1 Core Engine"
```
