use crate::agent_manager::AgentManager;
use easyjob_common::{ExecutionId, TaskId, TriggerId};
use easyjob_domain::execution::Execution;
use easyjob_domain::task::Task;
use easyjob_domain::SystemSettings;
use easyjob_ipc::protocol::AgentStatus;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use tauri::{Manager, State};

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

/// Export filename must be a plain `.json` file name, not a path.
pub fn validate_export_filename(filename: &str) -> Result<(), String> {
    if filename.is_empty() || filename.len() > 128 {
        return Err("导出文件名长度不合法".to_string());
    }
    if !filename.ends_with(".json") {
        return Err("导出文件必须是 .json".to_string());
    }
    if filename.starts_with('.') {
        return Err("导出文件名不能以 . 开头".to_string());
    }
    if !filename
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
    {
        return Err("导出文件名包含非法字符".to_string());
    }
    Ok(())
}

/// Build the platform-specific command that reveals a file in the file manager.
pub fn reveal_command_for_platform(platform: &str, path: &Path) -> (String, Vec<String>) {
    match platform {
        "macos" => (
            "open".to_string(),
            vec!["-R".to_string(), path.to_string_lossy().to_string()],
        ),
        "windows" => (
            "explorer".to_string(),
            vec![format!("/select,{}", path.display())],
        ),
        _ => {
            let parent = path.parent().unwrap_or(path);
            (
                "xdg-open".to_string(),
                vec![parent.to_string_lossy().to_string()],
            )
        }
    }
}

/// Write the exported task JSON into the user's Downloads directory.
#[tauri::command]
pub async fn export_tasks_json(
    app: tauri::AppHandle,
    filename: String,
    contents: String,
) -> Result<String, String> {
    validate_export_filename(&filename)?;
    let download_dir = app
        .path()
        .download_dir()
        .map_err(|e| format!("无法获取系统下载目录: {e}"))?;
    std::fs::create_dir_all(&download_dir).map_err(|e| format!("无法创建下载目录: {e}"))?;
    let path = download_dir.join(&filename);
    std::fs::write(&path, contents).map_err(|e| format!("写入导出文件失败: {e}"))?;
    Ok(path.to_string_lossy().to_string())
}

/// Reveal an exported file in Finder / Explorer (selecting the file).
#[tauri::command]
pub fn reveal_in_file_manager(path: String) -> Result<(), String> {
    let path = PathBuf::from(&path);
    if !path.is_file() {
        return Err("导出文件不存在".to_string());
    }
    let (program, args) = reveal_command_for_platform(std::env::consts::OS, &path);
    let status = Command::new(&program)
        .args(&args)
        .status()
        .map_err(|e| format!("无法打开文件管理器: {e}"))?;
    // Windows Explorer commonly exits with a non-zero code even after opening successfully.
    if status.success() || cfg!(target_os = "windows") {
        Ok(())
    } else {
        Err(format!("文件管理器返回失败状态: {program}"))
    }
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
