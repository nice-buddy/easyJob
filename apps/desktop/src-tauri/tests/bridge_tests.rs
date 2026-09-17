use easyjob_common::{ExecutionId, TaskId};
use easyjob_desktop_lib::agent_manager::AgentManager;
use easyjob_domain::execution::Execution;
use easyjob_domain::policy::ExecutionPolicy;
use easyjob_domain::task::Task;
use easyjob_ipc::client::IpcClient;
use easyjob_ipc::protocol::{AgentStatus, IpcEvent, IpcRequest, IpcResponse};
use easyjob_ipc::server::{IpcServer, RequestHandler};
use std::collections::HashMap;
use std::sync::Arc;
use tempfile::tempdir;

struct MockAgentHandler;

#[async_trait::async_trait]
impl RequestHandler for MockAgentHandler {
    async fn handle_request(&self, req: IpcRequest) -> IpcResponse {
        match req.method.as_str() {
            "agent.status" => IpcResponse::success(
                req.id,
                serde_json::to_value(AgentStatus {
                    version: "0.1.0".to_string(),
                    uptime_secs: 42,
                    active_tasks: 1,
                    running_executions: 0,
                })
                .unwrap(),
            ),
            "task.list" => IpcResponse::success(req.id, serde_json::json!([])),
            "task.get" => {
                let id: TaskId = serde_json::from_value(req.params["id"].clone())
                    .unwrap_or_else(|_| TaskId::new());
                let task = Task {
                    id,
                    name: "Test Task".to_string(),
                    description: None,
                    enabled: true,
                    triggers: vec![],
                    actions: vec![],
                    execution_policy: ExecutionPolicy::default(),
                    working_directory: None,
                    environment: HashMap::new(),
                    version: 1,
                    created_at: chrono::Utc::now(),
                    updated_at: chrono::Utc::now(),
                };
                IpcResponse::success(req.id, serde_json::to_value(task).unwrap())
            }
            "task.save" => {
                let task: Task = serde_json::from_value(req.params["task"].clone()).unwrap();
                IpcResponse::success(req.id, serde_json::to_value(task).unwrap())
            }
            "task.delete" => IpcResponse::success(req.id, serde_json::json!(true)),
            "task.trigger_now" => IpcResponse::success(req.id, serde_json::json!(true)),
            "execution.list" => IpcResponse::success(req.id, serde_json::json!([])),
            "execution.get" => {
                let id: ExecutionId = serde_json::from_value(req.params["id"].clone())
                    .unwrap_or_else(|_| ExecutionId::new());
                let task_id = TaskId::new();
                let mut exec = Execution::new(task_id, None, None);
                exec.id = id;
                IpcResponse::success(req.id, serde_json::to_value(exec).unwrap())
            }
            "execution.cancel" => IpcResponse::success(req.id, serde_json::json!(true)),
            "agent.shutdown" => IpcResponse::success(req.id, serde_json::json!(true)),
            _ => IpcResponse::error(req.id, format!("Method '{}' not found", req.method)),
        }
    }
}

#[tokio::test]
async fn test_ipc_bridge_client_calls() {
    let dir = tempdir().unwrap();
    let sock = dir.path().join("mock.sock");

    let server = IpcServer::bind(&sock, Arc::new(MockAgentHandler))
        .await
        .unwrap();
    tokio::spawn(server.run());

    let client = IpcClient::connect(&sock).await.unwrap();
    let res = client
        .call("agent.status", serde_json::json!({}))
        .await
        .unwrap();
    assert_eq!(res["version"], "0.1.0");
    assert_eq!(res["active_tasks"], 1);

    let trig = client
        .call(
            "task.trigger_now",
            serde_json::json!({ "id": TaskId::new() }),
        )
        .await
        .unwrap();
    assert_eq!(trig, serde_json::json!(true));
}

#[tokio::test]
async fn test_agent_manager_connection_and_caching() {
    let dir = tempdir().unwrap();
    let sock = dir.path().join("mock_mgr.sock");

    let server = IpcServer::bind(&sock, Arc::new(MockAgentHandler))
        .await
        .unwrap();
    tokio::spawn(server.run());

    let manager = AgentManager::with_ipc_path(sock.clone());
    let client1 = manager.ensure_connected().await.unwrap();
    let res: AgentStatus = serde_json::from_value(
        client1
            .call("agent.status", serde_json::json!({}))
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(res.version, "0.1.0");
    assert_eq!(res.active_tasks, 1);

    // Second call should reuse the same connection
    let client2 = manager.ensure_connected().await.unwrap();
    let tasks: Vec<Task> = serde_json::from_value(
        client2
            .call("task.list", serde_json::json!({}))
            .await
            .unwrap(),
    )
    .unwrap();
    assert!(tasks.is_empty());

    // Test disconnect and reconnect
    manager.disconnect().await;
    let client3 = manager.ensure_connected().await.unwrap();
    let del: bool = serde_json::from_value(
        client3
            .call("task.delete", serde_json::json!({ "id": TaskId::new() }))
            .await
            .unwrap(),
    )
    .unwrap();
    assert!(del);
}

#[tokio::test]
async fn test_all_rpc_methods() {
    let dir = tempdir().unwrap();
    let sock = dir.path().join("mock_rpc.sock");

    let server = IpcServer::bind(&sock, Arc::new(MockAgentHandler))
        .await
        .unwrap();
    tokio::spawn(server.run());

    let client = IpcClient::connect(&sock).await.unwrap();

    // 1. agent.status
    let status_val = client
        .call("agent.status", serde_json::json!({}))
        .await
        .unwrap();
    let status: AgentStatus = serde_json::from_value(status_val).unwrap();
    assert_eq!(status.version, "0.1.0");
    assert_eq!(status.uptime_secs, 42);

    // 2. task.list
    let tasks_val = client
        .call("task.list", serde_json::json!({}))
        .await
        .unwrap();
    let tasks: Vec<Task> = serde_json::from_value(tasks_val).unwrap();
    assert!(tasks.is_empty());

    // 3. task.get
    let task_id = TaskId::new();
    let task_val = client
        .call("task.get", serde_json::json!({ "id": task_id }))
        .await
        .unwrap();
    let task: Task = serde_json::from_value(task_val).unwrap();
    assert_eq!(task.id, task_id);
    assert_eq!(task.name, "Test Task");

    // 4. task.save
    let save_val = client
        .call("task.save", serde_json::json!({ "task": task }))
        .await
        .unwrap();
    let saved_task: Task = serde_json::from_value(save_val).unwrap();
    assert_eq!(saved_task.id, task_id);

    // 5. task.delete
    let del_val = client
        .call("task.delete", serde_json::json!({ "id": task_id }))
        .await
        .unwrap();
    let del_ok: bool = serde_json::from_value(del_val).unwrap();
    assert!(del_ok);

    // 6. task.trigger_now
    let trig_val = client
        .call("task.trigger_now", serde_json::json!({ "id": task_id }))
        .await
        .unwrap();
    let trig_ok: bool = serde_json::from_value(trig_val).unwrap();
    assert!(trig_ok);

    // 7. execution.list
    let execs_val = client
        .call("execution.list", serde_json::json!({ "limit": 10 }))
        .await
        .unwrap();
    let execs: Vec<Execution> = serde_json::from_value(execs_val).unwrap();
    assert!(execs.is_empty());

    // 8. execution.get
    let exec_id = ExecutionId::new();
    let exec_val = client
        .call("execution.get", serde_json::json!({ "id": exec_id }))
        .await
        .unwrap();
    let exec: Execution = serde_json::from_value(exec_val).unwrap();
    assert_eq!(exec.id, exec_id);

    // 9. execution.cancel
    let cancel_val = client
        .call("execution.cancel", serde_json::json!({ "id": exec_id }))
        .await
        .unwrap();
    let cancel_ok: bool = serde_json::from_value(cancel_val).unwrap();
    assert!(cancel_ok);

    // Unknown method returns error
    let err = client
        .call("unknown.method", serde_json::json!({}))
        .await
        .unwrap_err();
    assert!(err.to_string().contains("not found"));
}

#[tokio::test]
async fn test_agent_manager_event_subscription() {
    let dir = tempdir().unwrap();
    let sock = dir.path().join("mock_events.sock");

    let server = IpcServer::bind(&sock, Arc::new(MockAgentHandler))
        .await
        .unwrap();
    let event_tx = server.event_sender();
    tokio::spawn(server.run());

    let client = IpcClient::connect(&sock).await.unwrap();
    let mut rx = client.subscribe();

    // Verify connection is established and registered on server
    let _ = client
        .call("agent.status", serde_json::json!({}))
        .await
        .unwrap();

    // Broadcast an event from server via event_sender
    event_tx
        .send(IpcEvent::new(
            "task.updated",
            serde_json::json!({ "ok": true }),
        ))
        .unwrap();

    let event = tokio::time::timeout(std::time::Duration::from_secs(2), rx.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(event.event, "task.updated");
    assert_eq!(event.data, serde_json::json!({ "ok": true }));
}

#[tokio::test]
async fn test_agent_manager_call_helper() {
    let dir = tempdir().unwrap();
    let sock = dir.path().join("mock_call.sock");

    let server = IpcServer::bind(&sock, Arc::new(MockAgentHandler))
        .await
        .unwrap();
    tokio::spawn(server.run());

    let manager = AgentManager::with_ipc_path(sock.clone());
    let val = manager
        .call("agent.status", serde_json::json!({}))
        .await
        .unwrap();
    let status: AgentStatus = serde_json::from_value(val).unwrap();
    assert_eq!(status.version, "0.1.0");

    let err = manager
        .call("unknown.method", serde_json::json!({}))
        .await
        .unwrap_err();
    assert!(err.contains("not found"));
}

#[test]
fn test_minimized_arg_detection() {
    let args = vec!["easyjob-desktop".to_string(), "--minimized".to_string()];
    assert!(easyjob_desktop_lib::is_minimized_launch(&args));
    assert!(args.iter().any(|arg| arg == "--minimized"));

    let normal_args = vec!["easyjob-desktop".to_string()];
    assert!(!easyjob_desktop_lib::is_minimized_launch(&normal_args));
    assert!(!normal_args.iter().any(|arg| arg == "--minimized"));
}

#[test]
fn test_set_dock_visible_does_not_panic() {
    easyjob_desktop_lib::set_dock_visible(false);
    easyjob_desktop_lib::set_dock_visible(true);
}

#[tokio::test]
async fn test_agent_manager_shutdown_connected() {
    let dir = tempdir().unwrap();
    let sock = dir.path().join("mock_shutdown_connected.sock");

    let server = IpcServer::bind(&sock, Arc::new(MockAgentHandler))
        .await
        .unwrap();
    tokio::spawn(server.run());

    let manager = AgentManager::with_ipc_path(sock.clone());
    // First ensure connected so client guard is Some
    let _ = manager.ensure_connected().await.unwrap();
    assert!(manager.client_handle().lock().await.is_some());

    // Shutdown should invoke agent.shutdown and disconnect
    manager.shutdown_agent().await;
    assert!(manager.client_handle().lock().await.is_none());
}

#[tokio::test]
async fn test_agent_manager_shutdown_unconnected_running_server() {
    let dir = tempdir().unwrap();
    let sock = dir.path().join("mock_shutdown_unconnected.sock");

    let server = IpcServer::bind(&sock, Arc::new(MockAgentHandler))
        .await
        .unwrap();
    tokio::spawn(server.run());

    let manager = AgentManager::with_ipc_path(sock.clone());
    // Manager has NOT connected yet, client guard is None
    assert!(manager.client_handle().lock().await.is_none());

    // Shutdown connects directly via ipc_path and sends shutdown
    manager.shutdown_agent().await;
    assert!(manager.client_handle().lock().await.is_none());
}

#[tokio::test]
async fn test_agent_manager_shutdown_offline_does_not_panic() {
    let dir = tempdir().unwrap();
    let sock = dir.path().join("non_existent.sock");

    let manager = AgentManager::with_ipc_path(sock.clone());
    assert!(manager.client_handle().lock().await.is_none());

    // Should complete cleanly and not spawn agent or panic
    manager.shutdown_agent().await;
    assert!(manager.client_handle().lock().await.is_none());
}
