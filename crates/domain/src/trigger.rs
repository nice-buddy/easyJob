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
