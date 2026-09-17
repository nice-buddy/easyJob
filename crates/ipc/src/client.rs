use crate::protocol::{IpcEvent, IpcRequest, IpcResponse};
use crate::transport::MAX_FRAME_LENGTH;
use easyjob_common::{Error, Result};
use futures::{SinkExt, StreamExt};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{broadcast, mpsc, oneshot, Mutex};
use tokio_util::codec::{Framed, LinesCodec};
use tokio_util::sync::CancellationToken;
use tracing::warn;

#[derive(Clone)]
pub struct IpcClient {
    req_tx: mpsc::Sender<IpcRequest>,
    event_tx: broadcast::Sender<IpcEvent>,
    pending_responses: Arc<Mutex<HashMap<String, oneshot::Sender<IpcResponse>>>>,
    cancel_token: CancellationToken,
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

        let cancel_token = CancellationToken::new();

        let pending_clone = pending_responses.clone();
        let event_tx_clone = event_tx.clone();

        // Background writer
        let writer_token = cancel_token.clone();
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = writer_token.cancelled() => break,
                    msg = req_rx.recv() => {
                        match msg {
                            Some(req) => {
                                if let Ok(json) = serde_json::to_string(&req) {
                                    if sink.send(json).await.is_err() {
                                        break;
                                    }
                                }
                            }
                            None => break,
                        }
                    }
                }
            }
            writer_token.cancel();
        });

        // Background reader & demuxer
        let reader_token = cancel_token.clone();
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = reader_token.cancelled() => break,
                    result = stream.next() => {
                        match result {
                            Some(Ok(line)) => {
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
                            Some(Err(e)) => {
                                warn!("Error reading IPC frame: {:?}", e);
                                break;
                            }
                            None => break,
                        }
                    }
                }
            }
            reader_token.cancel();
            let mut map = pending_clone.lock().await;
            map.clear();
        });

        Ok(Self {
            req_tx,
            event_tx,
            pending_responses,
            cancel_token,
        })
    }

    pub fn is_closed(&self) -> bool {
        self.cancel_token.is_cancelled()
    }

    pub fn cancellation_token(&self) -> CancellationToken {
        self.cancel_token.clone()
    }

    pub async fn call(&self, method: &str, params: serde_json::Value) -> Result<serde_json::Value> {
        self.call_timeout(method, params, Duration::from_secs(30))
            .await
    }

    pub async fn call_timeout(
        &self,
        method: &str,
        params: serde_json::Value,
        timeout: Duration,
    ) -> Result<serde_json::Value> {
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

        match tokio::time::timeout(timeout, resp_rx).await {
            Ok(Ok(resp)) => {
                if resp.ok {
                    Ok(resp.data.unwrap_or(serde_json::Value::Null))
                } else {
                    Err(Error::Other(
                        resp.error.unwrap_or_else(|| "IPC error".to_string()),
                    ))
                }
            }
            Ok(Err(e)) => {
                let mut map = self.pending_responses.lock().await;
                map.remove(&id);
                Err(Error::Other(format!("IPC response channel closed: {}", e)))
            }
            Err(_) => {
                let mut map = self.pending_responses.lock().await;
                map.remove(&id);
                Err(Error::Other(format!(
                    "IPC request timed out after {:?}",
                    timeout
                )))
            }
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<IpcEvent> {
        self.event_tx.subscribe()
    }
}
