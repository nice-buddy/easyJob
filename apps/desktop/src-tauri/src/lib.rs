pub mod agent_manager;
pub mod commands;
pub mod events;
pub mod tray;

use agent_manager::AgentManager;
use commands::*;
use events::spawn_event_relay;
use std::sync::Arc;
use tauri::Manager;

/// Check if the CLI arguments contain `--minimized` (used by autostart silent launch).
pub fn is_minimized_launch(args: &[String]) -> bool {
    args.iter().any(|arg| arg == "--minimized")
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let agent_manager = Arc::new(AgentManager::new());
    let manager_for_events = agent_manager.clone();

    tauri::Builder::default()
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec!["--minimized"]),
        ))
        .manage(agent_manager)
        .setup(move |app| {
            let handle = app.handle().clone();
            spawn_event_relay(handle.clone(), manager_for_events);
            if let Err(e) = tray::setup_system_tray(&handle) {
                tracing::warn!("Failed to setup system tray: {:?}", e);
            }

            // 若存在 --minimized 参数（开机自启动唤醒），保持主窗口隐藏并静默常驻系统托盘
            let args: Vec<String> = std::env::args().collect();
            if is_minimized_launch(&args) {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.hide();
                }
            }

            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .invoke_handler(tauri::generate_handler![
            get_agent_status,
            list_tasks,
            get_task,
            save_task,
            delete_task,
            trigger_task,
            list_executions,
            get_execution,
            cancel_execution,
            get_execution_output,
        ])
        .run(tauri::generate_context!())
        .expect("error while running easyJob desktop");
}
