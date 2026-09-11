use crate::protocol::{IpcEvent, IpcRequest, IpcResponse};
use crate::transport::MAX_FRAME_LENGTH;
use easyjob_common::{Error, Result};
use futures::{SinkExt, StreamExt};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::{broadcast, mpsc, oneshot, Mutex};
use tokio_util::codec::{Framed, LinesCodec};
use tracing::warn;

#[derive(Clone)]
pub struct IpcClient {
    req_tx: mpsc::Sender<IpcRequest>,
    event_tx: broadcast::Sender<IpcEvent>,
    pending_responses: Arc<Mutex<HashMap<String, oneshot::Sender<IpcResponse>>>>,
}

impl IpcClient {
    pub async fn connect(path: &Path) -> Result<Self> {
        #[cfg(unix)]
        let stream = tokio::net::UnixStream::connect(path)
            .await
            .map_err(Error::Io)?;

        #[cfg(windows)]
        let stream = {
            use tokio::net::windows::named_pipe::ClientOptions;
            let pipe_name = path.to_string_lossy().to_string();
            ClientOptions::new().open(&pipe_name).map_err(Error::Io)?
        };

        let (mut sink, mut stream) =
            Framed::new(stream, LinesCodec::new_with_max_length(MAX_FRAME_LENGTH)).split();

        let (req_tx, mut req_rx) = mpsc::channel::<IpcRequest>(256);
        let (event_tx, _) = broadcast::channel::<IpcEvent>(1024);

        let pending_responses: Arc<Mutex<HashMap<String, oneshot::Sender<IpcResponse>>>> =
            Arc::new(Mutex::new(HashMap::new()));

        let pending_clone = pending_responses.clone();
        let event_tx_clone = event_tx.clone();

        // Background writer
        tokio::spawn(async move {
            while let Some(req) = req_rx.recv().await {
                if let Ok(json) = serde_json::to_string(&req) {
                    if sink.send(json).await.is_err() {
                        break;
                    }
                }
            }
        });

        // Background reader & demuxer
        tokio::spawn(async move {
            while let Some(result) = stream.next().await {
                match result {
                    Ok(line) => {
                        if let Ok(resp) = serde_json::from_str::<IpcResponse>(&line) {
                            let mut map = pending_clone.lock().await;
                            if let Some(ch) = map.remove(&resp.id) {
                                let _ = ch.send(resp);
                            }
                        } else if let Ok(event) = serde_json::from_str::<IpcEvent>(&line) {
                            let _ = event_tx_clone.send(event);
                        } else {
                            warn!("Received unrecognized IPC frame: {}", line);
                        }
                    }
                    Err(e) => {
                        warn!("Error reading IPC frame: {:?}", e);
                        break;
                    }
                }
            }
            let mut map = pending_clone.lock().await;
            map.clear();
        });

        Ok(Self {
            req_tx,
            event_tx,
            pending_responses,
        })
    }

    pub async fn call(&self, method: &str, params: serde_json::Value) -> Result<serde_json::Value> {
        let req = IpcRequest::new(method, params);
        let id = req.id.clone();
        let (resp_tx, resp_rx) = oneshot::channel();

        // Register pending response BEFORE sending request
        {
            let mut map = self.pending_responses.lock().await;
            map.insert(id.clone(), resp_tx);
        }

        if let Err(e) = self.req_tx.send(req).await {
            let mut map = self.pending_responses.lock().await;
            map.remove(&id);
            return Err(Error::Other(format!("Failed to send IPC request: {}", e)));
        }

        match resp_rx.await {
            Ok(resp) => {
                if resp.ok {
                    Ok(resp.data.unwrap_or(serde_json::Value::Null))
                } else {
                    Err(Error::Other(
                        resp.error.unwrap_or_else(|| "IPC error".to_string()),
                    ))
                }
            }
            Err(e) => {
                let mut map = self.pending_responses.lock().await;
                map.remove(&id);
                Err(Error::Other(format!("IPC response channel closed: {}", e)))
            }
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<IpcEvent> {
        self.event_tx.subscribe()
    }
}
