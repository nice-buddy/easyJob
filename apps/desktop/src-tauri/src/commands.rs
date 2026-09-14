use crate::agent_manager::AgentManager;
use easyjob_common::{ExecutionId, TaskId};
use easyjob_domain::execution::Execution;
use easyjob_domain::task::Task;
use easyjob_ipc::protocol::AgentStatus;
use tauri::State;

#[tauri::command]
pub async fn get_agent_status(manager: State<'_, AgentManager>) -> Result<AgentStatus, String> {
    let client = manager.ensure_connected().await?;
    let val = client
        .call("agent.status", serde_json::json!({}))
        .await
        .map_err(|e| e.to_string())?;
    serde_json::from_value(val).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn list_tasks(manager: State<'_, AgentManager>) -> Result<Vec<Task>, String> {
    let client = manager.ensure_connected().await?;
    let val = client
        .call("task.list", serde_json::json!({}))
        .await
        .map_err(|e| e.to_string())?;
    serde_json::from_value(val).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_task(id: TaskId, manager: State<'_, AgentManager>) -> Result<Task, String> {
    let client = manager.ensure_connected().await?;
    let val = client
        .call("task.get", serde_json::json!({ "id": id }))
        .await
        .map_err(|e| e.to_string())?;
    serde_json::from_value(val).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn save_task(task: Task, manager: State<'_, AgentManager>) -> Result<Task, String> {
    let client = manager.ensure_connected().await?;
    let val = client
        .call("task.save", serde_json::json!({ "task": task }))
        .await
        .map_err(|e| e.to_string())?;
    serde_json::from_value(val).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn delete_task(id: TaskId, manager: State<'_, AgentManager>) -> Result<bool, String> {
    let client = manager.ensure_connected().await?;
    let val = client
        .call("task.delete", serde_json::json!({ "id": id }))
        .await
        .map_err(|e| e.to_string())?;
    serde_json::from_value(val).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn trigger_task(id: TaskId, manager: State<'_, AgentManager>) -> Result<bool, String> {
    let client = manager.ensure_connected().await?;
    let val = client
        .call("task.trigger_now", serde_json::json!({ "id": id }))
        .await
        .map_err(|e| e.to_string())?;
    serde_json::from_value(val).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn list_executions(
    limit: Option<u32>,
    manager: State<'_, AgentManager>,
) -> Result<Vec<Execution>, String> {
    let client = manager.ensure_connected().await?;
    let val = client
        .call(
            "execution.list",
            serde_json::json!({ "limit": limit.unwrap_or(50) }),
        )
        .await
        .map_err(|e| e.to_string())?;
    serde_json::from_value(val).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_execution(
    id: ExecutionId,
    manager: State<'_, AgentManager>,
) -> Result<Execution, String> {
    let client = manager.ensure_connected().await?;
    let val = client
        .call("execution.get", serde_json::json!({ "id": id }))
        .await
        .map_err(|e| e.to_string())?;
    serde_json::from_value(val).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn cancel_execution(
    id: ExecutionId,
    manager: State<'_, AgentManager>,
) -> Result<bool, String> {
    let client = manager.ensure_connected().await?;
    let val = client
        .call("execution.cancel", serde_json::json!({ "id": id }))
        .await
        .map_err(|e| e.to_string())?;
    serde_json::from_value(val).map_err(|e| e.to_string())
}
