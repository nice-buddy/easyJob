use easyjob_ipc::client::IpcClient;
use easyjob_ipc::protocol::{IpcEvent, IpcRequest, IpcResponse};
use easyjob_ipc::server::{IpcServer, RequestHandler};
use easyjob_ipc::transport::default_ipc_path;
use std::sync::Arc;
use tempfile::tempdir;

struct EchoHandler;

#[async_trait::async_trait]
impl RequestHandler for EchoHandler {
    async fn handle_request(&self, req: IpcRequest) -> IpcResponse {
        if req.method == "ping" {
            IpcResponse::success(req.id, serde_json::json!("pong"))
        } else if req.method == "echo" {
            IpcResponse::success(req.id, req.params)
        } else {
            IpcResponse::error(req.id, "unknown method")
        }
    }
}

#[tokio::test]
async fn test_ipc_server_client_call_and_event() {
    let dir = tempdir().unwrap();
    let socket_path = dir.path().join("test_easyjob.sock");

    let server = IpcServer::bind(&socket_path, Arc::new(EchoHandler))
        .await
        .unwrap();
    let broadcast_tx = server.event_sender();
    tokio::spawn(server.run());

    let client = IpcClient::connect(&socket_path).await.unwrap();
    let mut event_rx = client.subscribe();

    // Test RPC Call
    let resp = client.call("ping", serde_json::json!({})).await.unwrap();
    assert_eq!(resp, serde_json::json!("pong"));

    let echo_resp = client
        .call("echo", serde_json::json!({ "foo": "bar" }))
        .await
        .unwrap();
    assert_eq!(echo_resp, serde_json::json!({ "foo": "bar" }));

    // Test Server Event Broadcast
    broadcast_tx
        .send(IpcEvent::new(
            "test.alert",
            serde_json::json!({ "msg": "hello" }),
        ))
        .unwrap();

    let received_event = tokio::time::timeout(std::time::Duration::from_secs(2), event_rx.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(received_event.event, "test.alert");
    assert_eq!(received_event.data, serde_json::json!({ "msg": "hello" }));

    // Test RPC error handling
    let err = client.call("unknown", serde_json::json!({})).await;
    assert!(err.is_err());
}

#[tokio::test]
async fn test_ipc_dead_socket_cleanup_and_permissions() {
    let dir = tempdir().unwrap();
    let socket_path = dir.path().join("dead_socket.sock");

    // Create a dead socket / dummy file
    std::fs::write(&socket_path, b"dead socket placeholder").unwrap();
    assert!(socket_path.exists());

    // Bind should remove the dead socket and bind successfully
    let server = IpcServer::bind(&socket_path, Arc::new(EchoHandler))
        .await
        .unwrap();
    tokio::spawn(server.run());

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let metadata = std::fs::metadata(&socket_path).unwrap();
        let mode = metadata.permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "Unix socket file permissions must be 0600");
    }

    let client = IpcClient::connect(&socket_path).await.unwrap();
    let resp = client.call("ping", serde_json::json!({})).await.unwrap();
    assert_eq!(resp, serde_json::json!("pong"));
}

#[tokio::test]
async fn test_ipc_multiple_concurrent_calls() {
    let dir = tempdir().unwrap();
    let socket_path = dir.path().join("concurrent.sock");

    let server = IpcServer::bind(&socket_path, Arc::new(EchoHandler))
        .await
        .unwrap();
    tokio::spawn(server.run());

    let client = Arc::new(IpcClient::connect(&socket_path).await.unwrap());

    let mut handles = Vec::new();
    for i in 0..10 {
        let c = client.clone();
        handles.push(tokio::spawn(async move {
            let res = c
                .call("echo", serde_json::json!({ "num": i }))
                .await
                .unwrap();
            assert_eq!(res, serde_json::json!({ "num": i }));
        }));
    }

    for h in handles {
        h.await.unwrap();
    }
}

#[test]
fn test_default_ipc_path() {
    let path = default_ipc_path();
    #[cfg(unix)]
    {
        assert!(path.to_string_lossy().ends_with(".easyjob/easyjob.sock"));
    }
    #[cfg(windows)]
    {
        assert!(path.to_string_lossy().starts_with(r"\\.\pipe\easyjob-"));
    }
}

#[tokio::test]
async fn test_ipc_large_payload() {
    let dir = tempdir().unwrap();
    let socket_path = dir.path().join("large_payload.sock");

    let server = IpcServer::bind(&socket_path, Arc::new(EchoHandler))
        .await
        .unwrap();
    tokio::spawn(server.run());

    let client = IpcClient::connect(&socket_path).await.unwrap();

    // 1MB payload string (well above default 8KB linescodec limit, well below 4MB limit)
    let large_string = "a".repeat(1024 * 1024);
    let resp = client
        .call("echo", serde_json::json!({ "payload": &large_string }))
        .await
        .unwrap();

    assert_eq!(
        resp.get("payload").and_then(|p| p.as_str()),
        Some(large_string.as_str())
    );
}

#[tokio::test]
async fn test_ipc_multiple_clients_broadcast_and_pruning() {
    let dir = tempdir().unwrap();
    let socket_path = dir.path().join("multi_client.sock");

    let server = IpcServer::bind(&socket_path, Arc::new(EchoHandler))
        .await
        .unwrap();
    let broadcast_tx = server.event_sender();
    tokio::spawn(server.run());

    let client1 = IpcClient::connect(&socket_path).await.unwrap();
    let mut rx1 = client1.subscribe();

    let client2 = IpcClient::connect(&socket_path).await.unwrap();
    let mut rx2 = client2.subscribe();

    // Broadcast 1: both clients receive
    broadcast_tx
        .send(IpcEvent::new("event.1", serde_json::json!({ "val": 1 })))
        .unwrap();

    let ev1 = tokio::time::timeout(std::time::Duration::from_secs(2), rx1.recv())
        .await
        .unwrap()
        .unwrap();
    let ev2 = tokio::time::timeout(std::time::Duration::from_secs(2), rx2.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(ev1.event, "event.1");
    assert_eq!(ev2.event, "event.1");

    // Drop client 1
    drop(client1);
    drop(rx1);

    // Broadcast 2: client 2 receives, dead client 1 pruned by server
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    broadcast_tx
        .send(IpcEvent::new("event.2", serde_json::json!({ "val": 2 })))
        .unwrap();

    let ev2_second = tokio::time::timeout(std::time::Duration::from_secs(2), rx2.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(ev2_second.event, "event.2");
}
