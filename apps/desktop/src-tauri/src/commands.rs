use crate::agent_manager::AgentManager;
use easyjob_common::{ExecutionId, TaskId};
use easyjob_domain::execution::Execution;
use easyjob_domain::task::Task;
use easyjob_ipc::protocol::AgentStatus;
use std::sync::Arc;
use tauri::State;

#[tauri::command]
pub async fn get_agent_status(
    manager: State<'_, Arc<AgentManager>>,
) -> Result<AgentStatus, String> {
    let val = manager.call("agent.status", serde_json::json!({})).await?;
    serde_json::from_value(val).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn list_tasks(manager: State<'_, Arc<AgentManager>>) -> Result<Vec<Task>, String> {
    let val = manager.call("task.list", serde_json::json!({})).await?;
    serde_json::from_value(val).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_task(id: TaskId, manager: State<'_, Arc<AgentManager>>) -> Result<Task, String> {
    let val = manager
        .call("task.get", serde_json::json!({ "id": id }))
        .await?;
    serde_json::from_value(val).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn save_task(task: Task, manager: State<'_, Arc<AgentManager>>) -> Result<Task, String> {
    let val = manager
        .call("task.save", serde_json::json!({ "task": task }))
        .await?;
    serde_json::from_value(val).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn delete_task(
    id: TaskId,
    manager: State<'_, Arc<AgentManager>>,
) -> Result<bool, String> {
    let val = manager
        .call("task.delete", serde_json::json!({ "id": id }))
        .await?;
    serde_json::from_value(val).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn trigger_task(
    id: TaskId,
    manager: State<'_, Arc<AgentManager>>,
) -> Result<bool, String> {
    let val = manager
        .call("task.trigger_now", serde_json::json!({ "id": id }))
        .await?;
    serde_json::from_value(val).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn list_executions(
    limit: Option<u32>,
    manager: State<'_, Arc<AgentManager>>,
) -> Result<Vec<Execution>, String> {
    let val = manager
        .call(
            "execution.list",
            serde_json::json!({ "limit": limit.unwrap_or(50) }),
        )
        .await?;
    serde_json::from_value(val).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_execution(
    id: ExecutionId,
    manager: State<'_, Arc<AgentManager>>,
) -> Result<Execution, String> {
    let val = manager
        .call("execution.get", serde_json::json!({ "id": id }))
        .await?;
    serde_json::from_value(val).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn cancel_execution(
    id: ExecutionId,
    manager: State<'_, Arc<AgentManager>>,
) -> Result<bool, String> {
    let val = manager
        .call("execution.cancel", serde_json::json!({ "id": id }))
        .await?;
    serde_json::from_value(val).map_err(|e| e.to_string())
}
