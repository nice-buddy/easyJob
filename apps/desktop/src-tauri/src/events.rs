use crate::agent_manager::AgentManager;
use std::sync::Arc;
use tauri::{AppHandle, Emitter};
use tracing::{error, info, warn};

pub fn spawn_event_relay(app_handle: AppHandle, manager: Arc<AgentManager>) {
    tokio::spawn(async move {
        loop {
            match manager.ensure_connected().await {
                Ok(client) => {
                    let mut rx = client.subscribe();
                    info!("Event relay listening to Agent IPC broadcast");
                    while let Ok(event) = rx.recv().await {
                        if let Err(e) = app_handle.emit(&event.event, &event.data) {
                            error!("Failed to emit Tauri event {}: {:?}", event.event, e);
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
