use easyjob_agent::service::AgentService;
use easyjob_common::{ActionId, TaskId, TriggerId};
use easyjob_domain::action::{Action, ActionKind};
use easyjob_domain::policy::ExecutionPolicy;
use easyjob_domain::task::Task;
use easyjob_domain::trigger::{Trigger, TriggerKind};
use easyjob_ipc::client::IpcClient;
use std::collections::HashMap;
use std::time::Duration;
use tempfile::tempdir;

#[tokio::test]
async fn test_agent_service_lifecycle_and_rpc() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("agent_test.db");
    let socket_path = dir.path().join("agent_test.sock");
    let db_url = format!("sqlite://{}?mode=rwc", db_path.to_string_lossy());

    let service = AgentService::init(&db_url, &socket_path, 4)
        .await
        .expect("AgentService::init failed");

    let service_handle = tokio::spawn(service.run());

    let client = IpcClient::connect(&socket_path)
        .await
        .expect("IpcClient::connect failed");
    let mut event_rx = client.subscribe();

    // 1. Check initial agent status
    let status_val = client
        .call("agent.status", serde_json::json!({}))
        .await
        .expect("agent.status failed");
    assert_eq!(status_val["version"], "0.1.0");
    assert_eq!(status_val["active_tasks"], 0);
    assert_eq!(status_val["running_executions"], 0);

    // 2. Check task list is initially empty
    let task_list = client
        .call("task.list", serde_json::json!({}))
        .await
        .expect("task.list failed");
    assert_eq!(task_list, serde_json::json!([]));

    // 3. Save a task with an action
    let task_id = TaskId::new();
    let action_id = ActionId::new();
    let trigger_id = TriggerId::new();

    let task = Task {
        id: task_id,
        name: "Test Echo Task".to_string(),
        description: Some("Integration test task".to_string()),
        enabled: true,
        triggers: vec![Trigger {
            id: trigger_id,
            task_id,
            enabled: true,
            kind: TriggerKind::AgentStarted,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        }],
        actions: vec![Action {
            id: action_id,
            task_id,
            sequence: 1,
            enabled: true,
            kind: ActionKind::ExecuteShell {
                command: "echo test_output_123".to_string(),
            },
        }],
        execution_policy: ExecutionPolicy::default(),
        working_directory: None,
        environment: HashMap::new(),
        version: 1,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    };

    let saved_val = client
        .call("task.save", serde_json::json!({ "task": task }))
        .await
        .expect("task.save failed");
    assert_eq!(saved_val["name"], "Test Echo Task");

    // 4. Get task by ID
    let fetched_val = client
        .call("task.get", serde_json::json!({ "id": task_id }))
        .await
        .expect("task.get failed");
    assert_eq!(fetched_val["id"], task_id.to_string());
    assert_eq!(fetched_val["name"], "Test Echo Task");

    // 5. Check task.list now has 1 task
    let task_list = client
        .call("task.list", serde_json::json!({}))
        .await
        .expect("task.list failed");
    assert_eq!(task_list.as_array().map(|a| a.len()), Some(1));

    // 6. Trigger task execution manually and collect events
    let trigger_res = client
        .call("task.trigger_now", serde_json::json!({ "id": task_id }))
        .await
        .expect("task.trigger_now failed");
    assert_eq!(trigger_res, serde_json::json!(true));

    // Wait for execution events
    let mut started_seen = false;
    let mut output_seen = false;
    let mut finished_seen = false;

    let timeout_duration = Duration::from_secs(5);
    let start_instant = std::time::Instant::now();

    while start_instant.elapsed() < timeout_duration {
        if let Ok(Ok(event)) =
            tokio::time::timeout(Duration::from_millis(500), event_rx.recv()).await
        {
            match event.event.as_str() {
                "execution.started" => {
                    assert_eq!(event.data["task_id"], task_id.to_string());
                    started_seen = true;
                }
                "execution.output" => {
                    let content = event.data["content"].as_str().unwrap_or_default();
                    if content.contains("test_output_123") {
                        output_seen = true;
                    }
                }
                "execution.finished" => {
                    assert_eq!(event.data["task_id"], task_id.to_string());
                    assert_eq!(event.data["status"], "Succeeded");
                    finished_seen = true;
                    break;
                }
                _ => {}
            }
        }
    }

    assert!(started_seen, "execution.started event was not received");
    assert!(output_seen, "execution.output event was not received");
    assert!(finished_seen, "execution.finished event was not received");

    // 7. Delete task
    let delete_res = client
        .call("task.delete", serde_json::json!({ "id": task_id }))
        .await
        .expect("task.delete failed");
    assert_eq!(delete_res, serde_json::json!(true));

    // Verify task is gone
    let get_after_delete = client
        .call("task.get", serde_json::json!({ "id": task_id }))
        .await;
    assert!(get_after_delete.is_err());

    // 8. Graceful shutdown
    let shutdown_res = client
        .call("agent.shutdown", serde_json::json!({}))
        .await
        .expect("agent.shutdown failed");
    assert_eq!(shutdown_res, serde_json::json!(true));

    // Verify service terminates cleanly
    let join_res = tokio::time::timeout(Duration::from_secs(5), service_handle).await;
    assert!(join_res.is_ok(), "Service did not shut down within timeout");
    assert!(join_res.unwrap().is_ok(), "Service task failed");
}
