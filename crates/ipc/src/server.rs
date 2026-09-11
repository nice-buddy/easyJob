use crate::protocol::{IpcEvent, IpcRequest, IpcResponse};
use crate::transport::MAX_FRAME_LENGTH;
use async_trait::async_trait;
use easyjob_common::{Error, Result};
use futures::{SinkExt, StreamExt};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::{broadcast, mpsc, Mutex};
use tokio_util::codec::{Framed, LinesCodec};
use tracing::{error, info, warn};

#[async_trait]
pub trait RequestHandler: Send + Sync {
    async fn handle_request(&self, req: IpcRequest) -> IpcResponse;
}

struct IpcServerInner {
    path: PathBuf,
    handler: Arc<dyn RequestHandler>,
    event_tx: broadcast::Sender<IpcEvent>,
    clients: Mutex<HashMap<u64, mpsc::Sender<String>>>,
    next_client_id: AtomicU64,
}

impl Drop for IpcServerInner {
    fn drop(&mut self) {
        #[cfg(unix)]
        {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

impl IpcServerInner {
    async fn handle_connection<S>(&self, stream: S)
    where
        S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Send + Unpin + 'static,
    {
        let client_id = self.next_client_id.fetch_add(1, Ordering::Relaxed);
        let (client_tx, mut client_rx) = mpsc::channel::<String>(256);

        {
            let mut clients = self.clients.lock().await;
            clients.insert(client_id, client_tx.clone());
        }

        let (mut sink, mut stream) =
            Framed::new(stream, LinesCodec::new_with_max_length(MAX_FRAME_LENGTH)).split();

        let writer_handle = tokio::spawn(async move {
            while let Some(line) = client_rx.recv().await {
                if sink.send(line).await.is_err() {
                    break;
                }
            }
        });

        while let Some(result) = stream.next().await {
            match result {
                Ok(line) => {
                    if let Ok(req) = serde_json::from_str::<IpcRequest>(&line) {
                        let handler = self.handler.clone();
                        let tx = client_tx.clone();
                        tokio::spawn(async move {
                            let resp = handler.handle_request(req).await;
                            if let Ok(resp_json) = serde_json::to_string(&resp) {
                                let _ = tx.send(resp_json).await;
                            }
                        });
                    } else {
                        warn!(
                            "Unrecognized message format from client {}: {}",
                            client_id, line
                        );
                    }
                }
                Err(e) => {
                    warn!("Error reading from client {}: {:?}", client_id, e);
                    break;
                }
            }
        }

        writer_handle.abort();
        let mut clients = self.clients.lock().await;
        clients.remove(&client_id);
    }
}

pub struct IpcServer {
    inner: Arc<IpcServerInner>,
    #[cfg(unix)]
    listener: tokio::net::UnixListener,
    #[cfg(windows)]
    pipe_instance: tokio::net::windows::named_pipe::NamedPipeServer,
}

impl IpcServer {
    pub async fn bind(path: &Path, handler: Arc<dyn RequestHandler>) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(Error::Io)?;
        }

        #[cfg(unix)]
        let listener = {
            if path.exists() {
                let _ = std::fs::remove_file(path);
            }
            let l = tokio::net::UnixListener::bind(path).map_err(Error::Io)?;
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
            l
        };

        #[cfg(windows)]
        let pipe_instance = {
            use tokio::net::windows::named_pipe::ServerOptions;
            let pipe_name = path.to_string_lossy().to_string();
            ServerOptions::new()
                .first_pipe_instance(true)
                .create(&pipe_name)
                .map_err(Error::Io)?
        };

        let (event_tx, _) = broadcast::channel(1024);

        let inner = Arc::new(IpcServerInner {
            path: path.to_path_buf(),
            handler,
            event_tx,
            clients: Mutex::new(HashMap::new()),
            next_client_id: AtomicU64::new(1),
        });

        Ok(Self {
            inner,
            #[cfg(unix)]
            listener,
            #[cfg(windows)]
            pipe_instance,
        })
    }

    pub fn event_sender(&self) -> broadcast::Sender<IpcEvent> {
        self.inner.event_tx.clone()
    }

    pub async fn run(self) -> Result<()> {
        let inner = self.inner;

        // Spawn event broadcast listener
        let broadcast_inner = inner.clone();
        let mut event_rx = broadcast_inner.event_tx.subscribe();
        let broadcast_handle = tokio::spawn(async move {
            while let Ok(event) = event_rx.recv().await {
                if let Ok(json) = serde_json::to_string(&event) {
                    let client_senders: Vec<(u64, mpsc::Sender<String>)> = {
                        let clients = broadcast_inner.clients.lock().await;
                        clients.iter().map(|(&id, tx)| (id, tx.clone())).collect()
                    };
                    let mut dead_clients = Vec::new();
                    for (id, tx) in client_senders {
                        if tx.try_send(json.clone()).is_err() {
                            dead_clients.push(id);
                        }
                    }
                    if !dead_clients.is_empty() {
                        let mut clients = broadcast_inner.clients.lock().await;
                        for id in dead_clients {
                            clients.remove(&id);
                        }
                    }
                }
            }
        });

        #[cfg(unix)]
        {
            let listener = self.listener;
            info!("IPC Server listening on Unix socket: {:?}", inner.path);

            loop {
                match listener.accept().await {
                    Ok((stream, _)) => {
                        let s = inner.clone();
                        tokio::spawn(async move {
                            s.handle_connection(stream).await;
                        });
                    }
                    Err(e) => {
                        error!("Unix socket accept error: {:?}", e);
                        break;
                    }
                }
            }
        }

        #[cfg(windows)]
        {
            use tokio::net::windows::named_pipe::ServerOptions;
            let pipe_name = inner.path.to_string_lossy().to_string();
            info!("IPC Server listening on Named Pipe: {}", pipe_name);

            let mut server_instance = self.pipe_instance;

            loop {
                if let Err(e) = server_instance.connect().await {
                    error!("Named pipe connect error: {:?}", e);
                    break;
                }
                let connected_client = server_instance;
                match ServerOptions::new().create(&pipe_name) {
                    Ok(next_instance) => {
                        server_instance = next_instance;
                    }
                    Err(e) => {
                        error!("Failed to create next pipe instance: {:?}", e);
                        break;
                    }
                }

                let s = inner.clone();
                tokio::spawn(async move {
                    s.handle_connection(connected_client).await;
                });
            }
        }

        broadcast_handle.abort();
        Ok(())
    }
}
