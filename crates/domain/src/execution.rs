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
    pub fn new(
        task_id: TaskId,
        trigger_id: Option<TriggerId>,
        scheduled_at: Option<DateTime<Utc>>,
    ) -> Self {
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
