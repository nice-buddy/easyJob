use easyjob_agent::service::{AgentRpcHandler, AgentService};
use easyjob_common::{ActionId, TaskId, TriggerId};
use easyjob_domain::action::{Action, ActionKind};
use easyjob_domain::policy::{ExecutionPolicy, TaskNotificationPolicy};
use easyjob_domain::task::Task;
use easyjob_domain::trigger::{Trigger, TriggerKind};
use easyjob_ipc::client::IpcClient;
use easyjob_ipc::protocol::IpcRequest;
use easyjob_ipc::server::RequestHandler;
use easyjob_persistence::{ExecutionRepository, SettingsRepository, TaskRepository};
use std::collections::HashMap;
use std::sync::Arc;
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
    // trigger_now now returns the created Execution object (not just `true`)
    assert_eq!(trigger_res["task_id"], task_id.to_string());
    assert_eq!(trigger_res["status"], "Running");
    let returned_exec_id = trigger_res["id"]
        .as_str()
        .expect("execution id in response");

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
    assert_eq!(exec_item["id"], returned_exec_id);
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

#[tokio::test]
async fn test_agent_service_task_completion_notification_policies() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("notify_test.db");
    let socket_path = dir.path().join("notify_test.sock");
    let db_url = format!("sqlite://{}?mode=rwc", db_path.to_string_lossy());

    let service = AgentService::init(&db_url, &socket_path, 2)
        .await
        .expect("AgentService::init failed");
    let service_handle = tokio::spawn(service.run());

    let client = IpcClient::connect(&socket_path)
        .await
        .expect("IpcClient::connect failed");
    let mut event_rx = client.subscribe();

    // 1. Task with TaskNotificationPolicy::All (Success path)
    let task_id_all = TaskId::new();
    let policy_all = ExecutionPolicy {
        notification: TaskNotificationPolicy::All,
        ..Default::default()
    };

    let task_all = Task {
        id: task_id_all,
        name: "Success Notify Task".to_string(),
        description: None,
        enabled: true,
        triggers: vec![],
        actions: vec![Action {
            id: ActionId::new(),
            task_id: task_id_all,
            sequence: 1,
            enabled: true,
            kind: ActionKind::ExecuteShell {
                command: "echo notify_all_ok".to_string(),
            },
        }],
        execution_policy: policy_all,
        working_directory: None,
        environment: HashMap::new(),
        version: 1,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    };

    client
        .call("task.save", serde_json::json!({ "task": task_all }))
        .await
        .expect("task.save failed");

    client
        .call("task.trigger_now", serde_json::json!({ "id": task_id_all }))
        .await
        .expect("task.trigger_now failed");

    let timeout_duration = Duration::from_secs(5);
    let mut success_finished = false;
    let start_instant = std::time::Instant::now();
    while start_instant.elapsed() < timeout_duration {
        if let Ok(Ok(event)) =
            tokio::time::timeout(Duration::from_millis(500), event_rx.recv()).await
        {
            if event.event == "execution.finished"
                && event.data["task_id"] == task_id_all.to_string()
            {
                assert_eq!(event.data["status"], "Succeeded");
                success_finished = true;
                break;
            }
        }
    }
    assert!(
        success_finished,
        "Task with Policy::All did not finish successfully"
    );

    // 2. Task with TaskNotificationPolicy::OnlyFailure (Failure path)
    let task_id_fail = TaskId::new();
    let policy_fail = ExecutionPolicy {
        notification: TaskNotificationPolicy::OnlyFailure,
        ..Default::default()
    };

    #[cfg(target_os = "windows")]
    let fail_cmd = "powershell -Command exit 1".to_string();
    #[cfg(not(target_os = "windows"))]
    let fail_cmd = "false".to_string();

    let task_fail = Task {
        id: task_id_fail,
        name: "Fail Notify Task".to_string(),
        description: None,
        enabled: true,
        triggers: vec![],
        actions: vec![Action {
            id: ActionId::new(),
            task_id: task_id_fail,
            sequence: 1,
            enabled: true,
            kind: ActionKind::ExecuteShell { command: fail_cmd },
        }],
        execution_policy: policy_fail,
        working_directory: None,
        environment: HashMap::new(),
        version: 1,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    };

    client
        .call("task.save", serde_json::json!({ "task": task_fail }))
        .await
        .expect("task.save failed");

    client
        .call(
            "task.trigger_now",
            serde_json::json!({ "id": task_id_fail }),
        )
        .await
        .expect("task.trigger_now failed");

    let mut fail_finished = false;
    let start_instant = std::time::Instant::now();
    while start_instant.elapsed() < timeout_duration {
        if let Ok(Ok(event)) =
            tokio::time::timeout(Duration::from_millis(500), event_rx.recv()).await
        {
            if event.event == "execution.finished"
                && event.data["task_id"] == task_id_fail.to_string()
            {
                assert_eq!(event.data["status"], "Failed");
                fail_finished = true;
                break;
            }
        }
    }
    assert!(
        fail_finished,
        "Task with Policy::OnlyFailure did not finish as Failed"
    );

    // 3. Task with TaskNotificationPolicy::None (None path)
    let task_id_none = TaskId::new();
    let policy_none = ExecutionPolicy {
        notification: TaskNotificationPolicy::None,
        ..Default::default()
    };

    let task_none = Task {
        id: task_id_none,
        name: "No Notify Task".to_string(),
        description: None,
        enabled: true,
        triggers: vec![],
        actions: vec![Action {
            id: ActionId::new(),
            task_id: task_id_none,
            sequence: 1,
            enabled: true,
            kind: ActionKind::ExecuteShell {
                command: "echo notify_none_ok".to_string(),
            },
        }],
        execution_policy: policy_none,
        working_directory: None,
        environment: HashMap::new(),
        version: 1,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    };

    client
        .call("task.save", serde_json::json!({ "task": task_none }))
        .await
        .expect("task.save failed");

    client
        .call(
            "task.trigger_now",
            serde_json::json!({ "id": task_id_none }),
        )
        .await
        .expect("task.trigger_now failed");

    let mut none_finished = false;
    let start_instant = std::time::Instant::now();
    while start_instant.elapsed() < timeout_duration {
        if let Ok(Ok(event)) =
            tokio::time::timeout(Duration::from_millis(500), event_rx.recv()).await
        {
            if event.event == "execution.finished"
                && event.data["task_id"] == task_id_none.to_string()
            {
                assert_eq!(event.data["status"], "Succeeded");
                none_finished = true;
                break;
            }
        }
    }
    assert!(
        none_finished,
        "Task with Policy::None did not finish successfully"
    );

    // 4. Shut down cleanly
    let _ = client.call("agent.shutdown", serde_json::json!({})).await;
    let _ = tokio::time::timeout(Duration::from_secs(5), service_handle).await;
}

#[tokio::test]
async fn test_execution_finished_event_contains_notification_metadata() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("metadata_test.db");
    let socket_path = dir.path().join("metadata_test.sock");
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
    let task = Task {
        id: task_id,
        name: "Test Metadata Task".to_string(),
        description: None,
        enabled: true,
        triggers: vec![],
        actions: vec![Action {
            id: ActionId::new(),
            task_id,
            sequence: 1,
            enabled: true,
            kind: ActionKind::ExecuteShell {
                command: "echo metadata_check".to_string(),
            },
        }],
        execution_policy: ExecutionPolicy {
            notification: TaskNotificationPolicy::All,
            ..Default::default()
        },
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

    let timeout_duration = Duration::from_secs(5);
    let mut finished_event = None;
    let start_instant = std::time::Instant::now();
    while start_instant.elapsed() < timeout_duration {
        if let Ok(Ok(event)) =
            tokio::time::timeout(Duration::from_millis(500), event_rx.recv()).await
        {
            if event.event == "execution.finished" && event.data["task_id"] == task_id.to_string() {
                finished_event = Some(event);
                break;
            }
        }
    }

    let event = finished_event.expect("execution.finished event not received");
    assert_eq!(event.data["task_id"], task_id.to_string());
    assert_eq!(event.data["task_name"], "Test Metadata Task");
    assert_eq!(event.data["status"], "Succeeded");
    assert_eq!(event.data["exit_code"], 0);
    assert!(event.data["duration_ms"].is_number());
    assert_eq!(event.data["notification_policy"], "All");

    let _ = client.call("agent.shutdown", serde_json::json!({})).await;
    let _ = tokio::time::timeout(Duration::from_secs(5), service_handle).await;
}

async fn setup_test_agent_handler() -> (
    AgentRpcHandler,
    std::sync::Arc<tokio::sync::Mutex<easyjob_scheduler::queue::ScheduleQueue>>,
    tempfile::TempDir,
) {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("agent_test.db");
    let db_url = format!("sqlite://{}?mode=rwc", db_path.to_string_lossy());
    let pool = easyjob_persistence::init_pool(&db_url).await.unwrap();

    let task_repo = Arc::new(easyjob_persistence::SqliteTaskRepository::new(pool.clone()));
    let exec_repo = Arc::new(easyjob_persistence::SqliteExecutionRepository::new(
        pool.clone(),
    ));
    let settings_repo = Arc::new(easyjob_persistence::SqliteSettingsRepository::new(
        pool.clone(),
    ));
    let exec_manager = Arc::new(easyjob_executor::manager::ExecutionManager::new(4));

    let (sched_event_tx, _sched_event_rx) =
        tokio::sync::mpsc::channel::<easyjob_scheduler::scheduler::TriggerEvent>(16);
    let (scheduler, scheduler_cmd_rx) =
        easyjob_scheduler::scheduler::Scheduler::new(sched_event_tx);
    let sched_queue = scheduler.queue();
    let scheduler_tx = scheduler.sender();
    let handle_queue = sched_queue.clone();
    let handle_event_tx = scheduler.event_sender();
    tokio::spawn(easyjob_scheduler::scheduler::Scheduler::run(
        handle_queue,
        scheduler_cmd_rx,
        handle_event_tx,
    ));

    let shutdown_notify = Arc::new(tokio::sync::Notify::new());
    let active_executions = Arc::new(tokio::sync::Mutex::new(HashMap::new()));
    let (event_tx, _) = tokio::sync::broadcast::channel(1024);
    let exec_cancel_token = tokio_util::sync::CancellationToken::new();

    let handler = AgentRpcHandler::new(
        task_repo,
        exec_repo,
        settings_repo,
        exec_manager,
        scheduler_tx,
        std::time::Instant::now(),
        shutdown_notify,
        active_executions,
        event_tx,
        exec_cancel_token,
        sched_queue.clone(),
    );

    (handler, sched_queue, dir)
}

#[tokio::test]
async fn test_agent_settings_ipc() {
    let (handler, _sched_queue, _dir) = setup_test_agent_handler().await;

    // 1. settings.get
    let req = IpcRequest::new("settings.get", serde_json::json!({}));
    let res = handler.handle_request(req).await;
    assert!(
        res.ok,
        "Expected settings.get to succeed, got: {:?}",
        res.error
    );
    let settings: easyjob_domain::SystemSettings =
        serde_json::from_value(res.data.unwrap()).unwrap();
    assert_eq!(
        settings.default_log_retention,
        easyjob_domain::SystemLogRetention::KeepDays(7)
    );

    // 2. settings.set
    let updated = easyjob_domain::SystemSettings {
        default_log_retention: easyjob_domain::SystemLogRetention::KeepDays(14),
    };
    let set_req = IpcRequest::new("settings.set", serde_json::json!({ "settings": updated }));
    let set_res = handler.handle_request(set_req).await;
    assert!(
        set_res.ok,
        "Expected settings.set to succeed, got: {:?}",
        set_res.error
    );

    // 3. 再次 get 验证更新生效
    let verify_req = IpcRequest::new("settings.get", serde_json::json!({}));
    let verify_res = handler.handle_request(verify_req).await;
    let current: easyjob_domain::SystemSettings =
        serde_json::from_value(verify_res.data.unwrap()).unwrap();
    assert_eq!(
        current.default_log_retention,
        easyjob_domain::SystemLogRetention::KeepDays(14)
    );
}

#[tokio::test]
async fn test_agent_log_retention_purge_on_task_completion() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("purge_test.db");
    let socket_path = dir.path().join("purge_test.sock");
    let db_url = format!("sqlite://{}?mode=rwc", db_path.to_string_lossy());

    let service = AgentService::init(&db_url, &socket_path, 2)
        .await
        .expect("AgentService::init failed");
    let exec_repo = service.execution_repository();
    let service_handle = tokio::spawn(service.run());

    let client = IpcClient::connect(&socket_path)
        .await
        .expect("IpcClient::connect failed");
    let mut event_rx = client.subscribe();

    // 1. Create a task with 3-day retention
    let task_id = TaskId::new();
    let task = Task {
        id: task_id,
        name: "Purge Test Task".to_string(),
        description: None,
        enabled: true,
        triggers: vec![],
        actions: vec![Action {
            id: ActionId::new(),
            task_id,
            sequence: 1,
            enabled: true,
            kind: ActionKind::ExecuteShell {
                command: "echo purge_test".to_string(),
            },
        }],
        execution_policy: ExecutionPolicy {
            log_retention: easyjob_domain::LogRetentionPolicy::KeepDays(3),
            ..Default::default()
        },
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

    // 2. Pre-insert an expired execution (10 days ago) and a recent one (1 day ago)
    let expired_exec_id = easyjob_common::ExecutionId::new();
    let mut expired_exec = easyjob_domain::execution::Execution::new(task_id, None, None);
    expired_exec.id = expired_exec_id;
    expired_exec.status = easyjob_domain::execution::ExecutionStatus::Succeeded;
    expired_exec.started_at = chrono::Utc::now() - chrono::Duration::days(10);
    expired_exec.finished_at = Some(chrono::Utc::now() - chrono::Duration::days(10));
    exec_repo.create_run(&expired_exec).await.unwrap();

    let recent_exec_id = easyjob_common::ExecutionId::new();
    let mut recent_exec = easyjob_domain::execution::Execution::new(task_id, None, None);
    recent_exec.id = recent_exec_id;
    recent_exec.status = easyjob_domain::execution::ExecutionStatus::Succeeded;
    recent_exec.started_at = chrono::Utc::now() - chrono::Duration::days(1);
    recent_exec.finished_at = Some(chrono::Utc::now() - chrono::Duration::days(1));
    exec_repo.create_run(&recent_exec).await.unwrap();

    // Verify both runs exist before triggering
    assert!(exec_repo
        .find_run_by_id(&expired_exec_id)
        .await
        .unwrap()
        .is_some());
    assert!(exec_repo
        .find_run_by_id(&recent_exec_id)
        .await
        .unwrap()
        .is_some());

    // 3. Trigger task manually
    let trigger_res = client
        .call("task.trigger_now", serde_json::json!({ "id": task_id }))
        .await
        .expect("task.trigger_now failed");
    let new_exec_id: easyjob_common::ExecutionId =
        serde_json::from_value(trigger_res["id"].clone()).unwrap();

    // 4. Wait for execution.finished event
    let timeout_duration = Duration::from_secs(5);
    let start_instant = std::time::Instant::now();
    let mut finished = false;
    while start_instant.elapsed() < timeout_duration {
        if let Ok(Ok(event)) =
            tokio::time::timeout(Duration::from_millis(500), event_rx.recv()).await
        {
            if event.event == "execution.finished" && event.data["task_id"] == task_id.to_string() {
                finished = true;
                break;
            }
        }
    }
    assert!(finished, "execution.finished not received");

    // 5. Poll for expired execution to be purged by the non-blocking background hook
    let purge_timeout = Duration::from_secs(5);
    let purge_start = std::time::Instant::now();
    let mut expired_purged = false;
    while purge_start.elapsed() < purge_timeout {
        if exec_repo
            .find_run_by_id(&expired_exec_id)
            .await
            .unwrap()
            .is_none()
        {
            expired_purged = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(
        expired_purged,
        "Expired execution run was not purged by hook"
    );

    // 6. Verify recent run and newly completed run still exist
    assert!(exec_repo
        .find_run_by_id(&recent_exec_id)
        .await
        .unwrap()
        .is_some());
    assert!(exec_repo
        .find_run_by_id(&new_exec_id)
        .await
        .unwrap()
        .is_some());

    // 7. Cleanup
    let _ = client.call("agent.shutdown", serde_json::json!({})).await;
    let _ = tokio::time::timeout(Duration::from_secs(5), service_handle).await;
}

#[tokio::test]
async fn test_trigger_log_retention_purge_helper_policies() {
    let (handler, _sched_queue, _dir) = setup_test_agent_handler().await;
    let exec_repo = handler.execution_repository();
    let settings_repo = handler.settings_repository();
    let task_repo = handler.task_repository();

    // Set system default to 5 days
    settings_repo
        .save_system_settings(&easyjob_domain::SystemSettings {
            default_log_retention: easyjob_domain::SystemLogRetention::KeepDays(5),
        })
        .await
        .unwrap();

    let task_id = TaskId::new();
    let mut task = Task {
        id: task_id,
        name: "Retention Helper Test".to_string(),
        description: None,
        enabled: true,
        triggers: vec![],
        actions: vec![],
        execution_policy: ExecutionPolicy {
            log_retention: easyjob_domain::LogRetentionPolicy::SystemDefault,
            ..Default::default()
        },
        working_directory: None,
        environment: HashMap::new(),
        version: 1,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    };
    task_repo.save(&task).await.unwrap();

    // Old run (7 days ago)
    let run1_id = easyjob_common::ExecutionId::new();
    let mut run1 = easyjob_domain::execution::Execution::new(task_id, None, None);
    run1.id = run1_id;
    run1.status = easyjob_domain::execution::ExecutionStatus::Succeeded;
    run1.started_at = chrono::Utc::now() - chrono::Duration::days(7);
    run1.finished_at = Some(chrono::Utc::now() - chrono::Duration::days(7));
    exec_repo.create_run(&run1).await.unwrap();

    // Trigger purge with SystemDefault (which resolves to 5 days)
    easyjob_agent::service::trigger_log_retention_purge(
        &task,
        settings_repo.clone(),
        exec_repo.clone(),
    );

    // Wait for background spawn
    let purge_timeout = Duration::from_secs(3);
    let purge_start = std::time::Instant::now();
    let mut run1_purged = false;
    while purge_start.elapsed() < purge_timeout {
        if exec_repo.find_run_by_id(&run1_id).await.unwrap().is_none() {
            run1_purged = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(30)).await;
    }
    assert!(run1_purged, "Expired execution run1 was not purged");

    // Now test Permanent policy: run older than 100 days should NOT be purged
    task.execution_policy.log_retention = easyjob_domain::LogRetentionPolicy::Permanent;
    let run2_id = easyjob_common::ExecutionId::new();
    let mut run2 = easyjob_domain::execution::Execution::new(task_id, None, None);
    run2.id = run2_id;
    run2.status = easyjob_domain::execution::ExecutionStatus::Succeeded;
    run2.started_at = chrono::Utc::now() - chrono::Duration::days(100);
    run2.finished_at = Some(chrono::Utc::now() - chrono::Duration::days(100));
    exec_repo.create_run(&run2).await.unwrap();

    easyjob_agent::service::trigger_log_retention_purge(
        &task,
        settings_repo.clone(),
        exec_repo.clone(),
    );
    tokio::time::sleep(Duration::from_millis(150)).await;
    assert!(
        exec_repo.find_run_by_id(&run2_id).await.unwrap().is_some(),
        "Permanent policy should not purge runs"
    );
}

#[tokio::test]
async fn test_task_overview_ipc_reports_next_fire_and_last_run() {
    let (handler, sched_queue, _dir) = setup_test_agent_handler().await;
    let task_repo = handler.task_repository();
    let exec_repo = handler.execution_repository();

    let task_id = TaskId::new();
    let daily_trigger_id = TriggerId::new();
    let network_trigger_id = TriggerId::new();
    let daily_time = chrono::NaiveTime::from_hms_opt(9, 0, 0).unwrap();
    let task = Task {
        id: task_id,
        name: "Overview Task".to_string(),
        description: None,
        enabled: true,
        triggers: vec![
            Trigger {
                id: daily_trigger_id,
                task_id,
                enabled: true,
                kind: TriggerKind::Daily {
                    time: daily_time,
                    timezone: "UTC".into(),
                },
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
            },
            Trigger {
                id: network_trigger_id,
                task_id,
                enabled: true,
                kind: TriggerKind::Network {
                    events: vec![easyjob_domain::trigger::NetworkEventKind::Connect],
                    network_name: None,
                },
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
            },
        ],
        actions: vec![],
        execution_policy: ExecutionPolicy::default(),
        working_directory: None,
        environment: HashMap::new(),
        version: 1,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    };
    task_repo.save(&task).await.unwrap();

    // 无触发器、无运行记录的任务
    let empty_task_id = TaskId::new();
    let empty_task = Task {
        id: empty_task_id,
        name: "Empty Overview Task".to_string(),
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
    task_repo.save(&empty_task).await.unwrap();

    // 队列中只放 Daily 触发器的条目（Network 触发器永远不会入队）
    let next_fire = chrono::DateTime::parse_from_rfc3339("2026-09-20T09:00:00Z")
        .unwrap()
        .with_timezone(&chrono::Utc);
    {
        let mut q = sched_queue.lock().await;
        q.push(easyjob_scheduler::queue::ScheduledItem {
            task_id,
            trigger_id: daily_trigger_id,
            next_fire_at: next_fire,
            generation: 1,
        });
    }

    let mut run = easyjob_domain::execution::Execution::new(task_id, Some(daily_trigger_id), None);
    run.status = easyjob_domain::execution::ExecutionStatus::Succeeded;
    run.started_at = chrono::Utc::now() - chrono::Duration::minutes(5);
    run.finished_at = Some(chrono::Utc::now());
    run.duration_ms = Some(1234);
    run.exit_code = Some(0);
    exec_repo.create_run(&run).await.unwrap();

    let res = handler
        .handle_request(IpcRequest::new("task.overview", serde_json::json!({})))
        .await;
    assert!(res.ok, "task.overview failed: {:?}", res.error);
    let data = res.data.unwrap();
    let entries = data.as_array().expect("overview must be an array");

    let entry = entries
        .iter()
        .find(|e| e["task_id"] == task_id.to_string())
        .expect("task entry present");
    let triggers = entry["triggers"].as_array().expect("triggers array");
    assert_eq!(triggers.len(), 2);

    let daily = triggers
        .iter()
        .find(|t| t["trigger_id"] == daily_trigger_id.to_string())
        .expect("daily trigger present");
    let daily_next: chrono::DateTime<chrono::Utc> =
        serde_json::from_value(daily["next_fire_at"].clone()).unwrap();
    assert_eq!(daily_next, next_fire);

    let network = triggers
        .iter()
        .find(|t| t["trigger_id"] == network_trigger_id.to_string())
        .expect("network trigger present");
    assert!(
        network["next_fire_at"].is_null(),
        "network trigger has no queued entry"
    );

    let last_run = &entry["last_run"];
    assert_eq!(last_run["status"], "Succeeded");
    assert_eq!(last_run["duration_ms"], 1234);
    assert_eq!(last_run["exit_code"], 0);
    assert!(last_run["error_message"].is_null());

    let empty_entry = entries
        .iter()
        .find(|e| e["task_id"] == empty_task_id.to_string())
        .expect("empty task entry present");
    assert_eq!(empty_entry["triggers"].as_array().map(|a| a.len()), Some(0));
    assert!(
        empty_entry["last_run"].is_null(),
        "never-executed task must report last_run = null"
    );
}
