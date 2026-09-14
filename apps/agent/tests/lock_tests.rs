use easyjob_agent::lock::{LockOutcome, SingleInstanceLock};
use easyjob_ipc::protocol::{IpcRequest, IpcResponse};
use easyjob_ipc::server::{IpcServer, RequestHandler};
use std::sync::Arc;
use tempfile::tempdir;

struct MockAgentHandler;

#[async_trait::async_trait]
impl RequestHandler for MockAgentHandler {
    async fn handle_request(&self, req: IpcRequest) -> IpcResponse {
        if req.method == "agent.status" {
            IpcResponse::success(
                req.id,
                serde_json::json!({
                    "version": "0.1.0",
                    "uptime_secs": 10,
                    "active_tasks": 0,
                    "running_executions": 0
                }),
            )
        } else {
            IpcResponse::error(req.id, "unsupported method")
        }
    }
}

#[tokio::test]
async fn test_single_instance_lock_acquire_and_drop() {
    let dir = tempdir().unwrap();
    let lock_path = dir.path().join("agent.lock");
    let ipc_path = dir.path().join("agent.sock");

    let lock1 = SingleInstanceLock::acquire(&lock_path, &ipc_path)
        .await
        .expect("First lock acquire should succeed");
    assert!(matches!(lock1, LockOutcome::Acquired(_)));

    // Clean release on drop
    drop(lock1);

    // After drop, subsequent acquire should succeed cleanly
    let lock2 = SingleInstanceLock::acquire(&lock_path, &ipc_path)
        .await
        .expect("Lock acquire after drop should succeed");
    assert!(matches!(lock2, LockOutcome::Acquired(_)));
}

#[tokio::test]
async fn test_second_lock_returns_already_running_when_ipc_active() {
    let dir = tempdir().unwrap();
    let lock_path = dir.path().join("agent.lock");
    let ipc_path = dir.path().join("agent.sock");

    // Start mock IPC server
    let server = IpcServer::bind(&ipc_path, Arc::new(MockAgentHandler))
        .await
        .expect("Failed to bind mock IPC server");
    tokio::spawn(server.run());

    // Acquire lock for active agent
    let lock1 = SingleInstanceLock::acquire(&lock_path, &ipc_path)
        .await
        .expect("First acquire should succeed");
    assert!(matches!(lock1, LockOutcome::Acquired(_)));

    // Second lock attempt with active IPC server must report AlreadyRunning
    let lock2 = SingleInstanceLock::acquire(&lock_path, &ipc_path)
        .await
        .expect("Second acquire check should succeed");
    assert!(matches!(lock2, LockOutcome::AlreadyRunning));
}

#[tokio::test]
async fn test_orphaned_lock_file_acquired_successfully() {
    let dir = tempdir().unwrap();
    let lock_path = dir.path().join("agent.lock");
    let ipc_path = dir.path().join("agent.sock");

    // Simulate an orphaned lock file left on disk after prior daemon exited/crashed
    std::fs::write(&lock_path, "stale pid").unwrap();

    // New agent startup: lock file exists on disk, but kernel lock is released.
    // Acquire must succeed cleanly without contention.
    let lock = SingleInstanceLock::acquire(&lock_path, &ipc_path)
        .await
        .expect("Orphaned lock file should be acquired successfully");
    assert!(matches!(lock, LockOutcome::Acquired(_)));
}
