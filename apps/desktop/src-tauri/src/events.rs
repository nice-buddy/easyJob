use crate::agent_manager::AgentManager;
use std::sync::Arc;
use tauri::{AppHandle, Emitter};
use tauri_plugin_notification::NotificationExt;
use tokio::sync::broadcast::error::RecvError;
use tracing::{error, info, warn};

pub fn should_notify(policy: &str, status: &str) -> bool {
    let is_success = status == "Succeeded";
    match policy {
        "OnlySuccess" => is_success,
        "OnlyFailure" => !is_success,
        "All" => true,
        _ => false,
    }
}

pub fn format_notification_content(
    status: &str,
    task_name: &str,
    duration_ms: u64,
    error_message: Option<&str>,
) -> (&'static str, String) {
    let is_success = status == "Succeeded";
    let duration_str = if duration_ms < 1000 {
        format!("{}ms", duration_ms)
    } else {
        format!("{:.2}s", duration_ms as f64 / 1000.0)
    };

    let title = if is_success {
        "easyJob - 任务执行成功"
    } else {
        "easyJob - 任务执行失败"
    };

    let mut body = format!("任务「{}」耗时: {}", task_name, duration_str);
    if !is_success {
        if let Some(err) = error_message {
            let truncated: String = err.chars().take(60).collect();
            body.push_str(&format!(" | 错误: {}", truncated));
        }
    }

    (title, body)
}

pub fn spawn_event_relay(app_handle: AppHandle, manager: Arc<AgentManager>) {
    tauri::async_runtime::spawn(async move {
        loop {
            match manager.ensure_connected().await {
                Ok(client) => {
                    let mut rx = client.subscribe();
                    info!("Event relay listening to Agent IPC broadcast");
                    loop {
                        match rx.recv().await {
                            Ok(event) => {
                                if let Err(e) = app_handle.emit(&event.event, &event.data) {
                                    error!("Failed to emit Tauri event {}: {:?}", event.event, e);
                                }

                                if event.event == "execution.finished" {
                                    if let Some(policy) = event
                                        .data
                                        .get("notification_policy")
                                        .and_then(|v| v.as_str())
                                    {
                                        let status = event
                                            .data
                                            .get("status")
                                            .and_then(|v| v.as_str())
                                            .unwrap_or("");
                                        if should_notify(policy, status) {
                                            let task_name = event
                                                .data
                                                .get("task_name")
                                                .and_then(|v| v.as_str())
                                                .unwrap_or("未命名任务");
                                            let duration_ms = event
                                                .data
                                                .get("duration_ms")
                                                .and_then(|v| v.as_u64())
                                                .unwrap_or(0);
                                            let error_message = event
                                                .data
                                                .get("error_message")
                                                .and_then(|v| v.as_str());
                                            let (title, body) = format_notification_content(
                                                status,
                                                task_name,
                                                duration_ms,
                                                error_message,
                                            );

                                            let _ = app_handle
                                                .notification()
                                                .builder()
                                                .title(title)
                                                .body(body)
                                                .show();
                                        }
                                    }
                                }
                            }
                            Err(RecvError::Lagged(n)) => {
                                warn!("Event relay lagged by {} messages", n);
                                continue;
                            }
                            Err(RecvError::Closed) => {
                                info!("Agent event broadcast channel closed");
                                break;
                            }
                        }
                    }
                    warn!("Event relay subscription ended, attempting reconnect");
                    manager.disconnect().await;
                    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                }
                Err(_) => {
                    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_should_notify_policy() {
        assert!(should_notify("OnlySuccess", "Succeeded"));
        assert!(!should_notify("OnlySuccess", "Failed"));

        assert!(!should_notify("OnlyFailure", "Succeeded"));
        assert!(should_notify("OnlyFailure", "Failed"));

        assert!(should_notify("All", "Succeeded"));
        assert!(should_notify("All", "Failed"));

        assert!(!should_notify("None", "Succeeded"));
        assert!(!should_notify("None", "Failed"));
    }

    #[test]
    fn test_format_notification_content_success() {
        let (title, body) = format_notification_content("Succeeded", "备份数据", 850, None);
        assert_eq!(title, "easyJob - 任务执行成功");
        assert_eq!(body, "任务「备份数据」耗时: 850ms");

        let (title_sec, body_sec) =
            format_notification_content("Succeeded", "打包日志", 2500, None);
        assert_eq!(title_sec, "easyJob - 任务执行成功");
        assert_eq!(body_sec, "任务「打包日志」耗时: 2.50s");
    }

    #[test]
    fn test_format_notification_content_failure() {
        let (title, body) = format_notification_content(
            "Failed",
            "同步任务",
            1200,
            Some("Network timeout connection refused to server host"),
        );
        assert_eq!(title, "easyJob - 任务执行失败");
        assert!(body.contains("任务「同步任务」耗时: 1.20s"));
        assert!(body.contains(" | 错误: Network timeout"));
    }
}
