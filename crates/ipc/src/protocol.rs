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
