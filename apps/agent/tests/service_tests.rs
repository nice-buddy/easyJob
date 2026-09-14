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

    // 6b. Verify execution.list and execution.get
    let exec_list = client
        .call("execution.list", serde_json::json!({ "limit": 10 }))
        .await
        .expect("execution.list failed");
    let exec_arr = exec_list.as_array().expect("expected execution list array");
    assert!(!exec_arr.is_empty());
    let exec_id = exec_arr[0]["id"].as_str().expect("valid execution id");

    let exec_item = client
        .call("execution.get", serde_json::json!({ "id": exec_id }))
        .await
        .expect("execution.get failed");
    assert_eq!(exec_item["id"], exec_id);
    assert_eq!(exec_item["status"], "Succeeded");
    assert_eq!(exec_item["exit_code"], 0);

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

#[tokio::test]
async fn test_agent_service_global_concurrency_limit() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("concurrency_test.db");
    let socket_path = dir.path().join("concurrency_test.sock");
    let db_url = format!("sqlite://{}?mode=rwc", db_path.to_string_lossy());

    // Initialize with max_concurrent = 1
    let service = AgentService::init(&db_url, &socket_path, 1)
        .await
        .expect("AgentService::init failed");

    let global_limiter = service.execution_manager().global_semaphore();
    assert_eq!(global_limiter.available_permits(), 1);

    let service_handle = tokio::spawn(service.run());
    let client = IpcClient::connect(&socket_path)
        .await
        .expect("IpcClient::connect failed");

    // Shut down cleanly
    let _ = client.call("agent.shutdown", serde_json::json!({})).await;
    let _ = tokio::time::timeout(Duration::from_secs(5), service_handle).await;
}

#[tokio::test]
async fn test_agent_service_execution_cancel() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("cancel_test.db");
    let socket_path = dir.path().join("cancel_test.sock");
    let db_url = format!("sqlite://{}?mode=rwc", db_path.to_string_lossy());

    let service = AgentService::init(&db_url, &socket_path, 2)
        .await
        .expect("AgentService::init failed");
    let service_handle = tokio::spawn(service.run());

    let client = IpcClient::connect(&socket_path)
        .await
        .expect("IpcClient::connect failed");
    let mut event_rx = client.subscribe();

    let task_id = TaskId::new();
    let action_id = ActionId::new();
    let trigger_id = TriggerId::new();

    #[cfg(target_os = "windows")]
    let sleep_cmd = "powershell -Command Start-Sleep -Seconds 10".to_string();
    #[cfg(not(target_os = "windows"))]
    let sleep_cmd = "sleep 10".to_string();

    let task = Task {
        id: task_id,
        name: "Long Running Task".to_string(),
        description: None,
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
            kind: ActionKind::ExecuteShell { command: sleep_cmd },
        }],
        execution_policy: ExecutionPolicy::default(),
        working_directory: None,
        environment: HashMap::new(),
        version: 1,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    };

    client
        .call("task.save", serde_json::json!({ "task": task }))
        .await
        .expect("task.save failed");

    client
        .call("task.trigger_now", serde_json::json!({ "id": task_id }))
        .await
        .expect("task.trigger_now failed");

    // Wait for execution.started
    let mut execution_id = None;
    let timeout_duration = Duration::from_secs(5);
    let start_instant = std::time::Instant::now();
    while start_instant.elapsed() < timeout_duration {
        if let Ok(Ok(event)) =
            tokio::time::timeout(Duration::from_millis(500), event_rx.recv()).await
        {
            if event.event == "execution.started" {
                execution_id = Some(event.data["execution_id"].as_str().unwrap().to_string());
                break;
            }
        }
    }
    let exec_id = execution_id.expect("expected execution.started event");

    // Call execution.cancel
    let cancel_res = client
        .call("execution.cancel", serde_json::json!({ "id": exec_id }))
        .await
        .expect("execution.cancel failed");
    assert_eq!(cancel_res, serde_json::json!(true));

    // Wait for execution.finished with Cancelled status
    let mut cancelled_seen = false;
    let start_instant = std::time::Instant::now();
    while start_instant.elapsed() < timeout_duration {
        if let Ok(Ok(event)) =
            tokio::time::timeout(Duration::from_millis(500), event_rx.recv()).await
        {
            if event.event == "execution.finished" {
                assert_eq!(event.data["status"], "Cancelled");
                cancelled_seen = true;
                break;
            }
        }
    }
    assert!(
        cancelled_seen,
        "execution.finished Cancelled event not received"
    );

    // Verify execution.get confirms status == "Cancelled"
    let fetched = client
        .call("execution.get", serde_json::json!({ "id": exec_id }))
        .await
        .expect("execution.get failed");
    assert_eq!(fetched["status"], "Cancelled");

    // Shut down cleanly
    let _ = client.call("agent.shutdown", serde_json::json!({})).await;
    let _ = tokio::time::timeout(Duration::from_secs(5), service_handle).await;
}
