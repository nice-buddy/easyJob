use crate::agent_manager::AgentManager;
use easyjob_common::{ExecutionId, TaskId, TriggerId};
use easyjob_domain::execution::Execution;
use easyjob_domain::task::Task;
use easyjob_domain::SystemSettings;
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
) -> Result<Execution, String> {
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

#[tauri::command]
pub async fn get_execution_output(
    id: ExecutionId,
    manager: State<'_, Arc<AgentManager>>,
) -> Result<serde_json::Value, String> {
    manager
        .call("execution.get_output", serde_json::json!({ "id": id }))
        .await
}

#[tauri::command]
pub async fn restart_agent(manager: State<'_, Arc<AgentManager>>) -> Result<bool, String> {
    manager.restart_agent().await
}

#[tauri::command]
pub async fn get_system_settings(
    manager: State<'_, Arc<AgentManager>>,
) -> Result<SystemSettings, String> {
    let val = manager.call("settings.get", serde_json::json!({})).await?;
    serde_json::from_value(val).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn save_system_settings(
    settings: SystemSettings,
    manager: State<'_, Arc<AgentManager>>,
) -> Result<SystemSettings, String> {
    let val = manager
        .call("settings.set", serde_json::json!({ "settings": settings }))
        .await?;
    serde_json::from_value(val).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn task_overview(
    manager: State<'_, Arc<AgentManager>>,
) -> Result<serde_json::Value, String> {
    manager.call("task.overview", serde_json::json!({})).await
}

#[tauri::command]
pub async fn reroll_trigger(
    task_id: TaskId,
    trigger_id: TriggerId,
    manager: State<'_, Arc<AgentManager>>,
) -> Result<serde_json::Value, String> {
    manager
        .call(
            "trigger.reroll",
            serde_json::json!({ "task_id": task_id, "trigger_id": trigger_id }),
        )
        .await
}

#[tauri::command]
pub fn open_external_url(url: String) -> Result<(), String> {
    // 仅允许跳转到本项目的 GitHub Release 页面，避免任意 URL 带来的 open-redirect 风险
    const ALLOWED_PREFIX: &str = "https://github.com/nice-buddy/easyJob/releases";
    if !url.starts_with(ALLOWED_PREFIX) {
        return Err("不支持的链接".to_string());
    }
    open::that(&url).map_err(|e| e.to_string())
}
