pub mod agent_manager;
pub mod commands;
pub mod events;
pub mod tray;

use agent_manager::AgentManager;
use commands::*;
use events::spawn_event_relay;
use std::sync::Arc;
use tauri::{Emitter, Manager};

/// Check if the CLI arguments contain `--minimized` (used by autostart silent launch).
pub fn is_minimized_launch(args: &[String]) -> bool {
    args.iter().any(|arg| arg == "--minimized")
}

/// Show or hide the macOS Dock icon dynamically based on window visibility.
#[cfg(target_os = "macos")]
pub fn set_dock_visible(visible: bool) {
    use objc2::msg_send;
    use objc2::runtime::{AnyClass, AnyObject};

    if let Some(cls) = AnyClass::get(c"NSApplication") {
        unsafe {
            let app: *mut AnyObject = msg_send![cls, sharedApplication];
            if !app.is_null() {
                // 0 = NSApplicationActivationPolicyRegular (shows in Dock)
                // 1 = NSApplicationActivationPolicyAccessory (hidden from Dock, tray only)
                let policy: isize = if visible { 0 } else { 1 };
                let _: bool = msg_send![app, setActivationPolicy: policy];
                if visible {
                    let _: () = msg_send![app, activateIgnoringOtherApps: true];
                }
            }
        }
    }
}

#[cfg(not(target_os = "macos"))]
pub fn set_dock_visible(_visible: bool) {}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let agent_manager = Arc::new(AgentManager::new());
    let manager_for_events = agent_manager.clone();
    let manager_for_exit = agent_manager.clone();

    tauri::Builder::default()
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec!["--minimized"]),
        ))
        .plugin(tauri_plugin_notification::init())
        .manage(agent_manager)
        .setup(move |app| {
            let handle = app.handle().clone();
            spawn_event_relay(handle.clone(), manager_for_events);
            if let Err(e) = tray::setup_system_tray(&handle) {
                tracing::warn!("Failed to setup system tray: {:?}", e);
            }

            // 若存在 --minimized 参数（开机自启动唤醒），保持主窗口隐藏、隐藏 Dock 图标并静默常驻系统托盘
            let args: Vec<String> = std::env::args().collect();
            if is_minimized_launch(&args) {
                set_dock_visible(false);
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
                set_dock_visible(false);
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
            restart_agent,
        ])
        .build(tauri::generate_context!())
        .expect("error while building easyJob desktop")
        .run(move |app_handle, event| match event {
            tauri::RunEvent::ExitRequested { .. } => {
                tracing::info!("Application exit requested, shutting down agent daemon...");
                let manager = manager_for_exit.clone();
                tauri::async_runtime::block_on(async move {
                    manager.shutdown_agent().await;
                    tokio::time::sleep(std::time::Duration::from_millis(150)).await;
                });
            }
            tauri::RunEvent::Reopen { .. } => {
                set_dock_visible(true);
                if let Some(window) = app_handle.get_webview_window("main") {
                    let _ = window.show();
                    let _ = window.set_focus();
                    let _ = window.emit("navigate", "executions");
                }
            }
            _ => {}
        });
}
