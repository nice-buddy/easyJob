use easyjob_common::{ActionId, TaskId};
use serde::{Deserialize, Serialize};

/// 脚本输出编码。None 表示平台默认：Windows 下为 GBK（936），其他平台为 UTF-8。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ScriptEncoding {
    #[serde(rename = "utf8")]
    Utf8,
    #[serde(rename = "gbk")]
    Gbk,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ActionKind {
    ExecuteProgram { program: String, args: Vec<String> },
    ExecuteShell { command: String },
    ExecutePowerShell {
        script: String,
        no_profile: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        encoding: Option<ScriptEncoding>,
    },
    ExecuteCmd {
        command: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        encoding: Option<ScriptEncoding>,
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
