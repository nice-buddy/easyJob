use crate::agent_manager::AgentManager;
use std::sync::Arc;
use tauri::{AppHandle, Emitter};
use tokio::sync::broadcast::error::RecvError;
use tracing::{error, info, warn};

pub fn spawn_event_relay(app_handle: AppHandle, manager: Arc<AgentManager>) {
    tokio::spawn(async move {
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
