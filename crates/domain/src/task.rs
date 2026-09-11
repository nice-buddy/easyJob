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
