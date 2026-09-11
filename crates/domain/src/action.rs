use easyjob_common::{ActionId, TaskId};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ActionKind {
    ExecuteProgram { program: String, args: Vec<String> },
    ExecuteShell { command: String },
    ExecutePowerShell { script: String, no_profile: bool },
    ExecuteCmd { command: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Action {
    pub id: ActionId,
    pub task_id: TaskId,
    pub sequence: u32,
    pub enabled: bool,
    pub kind: ActionKind,
}
