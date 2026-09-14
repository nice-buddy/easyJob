use easyjob_agent::service::AgentService;
use easyjob_common::{ActionId, TaskId, TriggerId};
use easyjob_domain::action::{Action, ActionKind};
use easyjob_domain::policy::ExecutionPolicy;
use easyjob_domain::task::Task;
use easyjob_domain::trigger::{Trigger, TriggerKind};
use easyjob_ipc::client::IpcClient;
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;
use tempfile::tempdir;

#[tokio::test]
async fn test_full_agent_daemon_ipc_lifecycle() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("daemon.db");
    #[cfg(target_os = "windows")]
    let socket_path = PathBuf::from(format!(
        r"\\.\pipe\easyjob-test-daemon-{}",
        uuid::Uuid::new_v4()
    ));
    #[cfg(not(target_os = "windows"))]
    let socket_path = dir.path().join("daemon.sock");
    let db_url = format!("sqlite://{}?mode=rwc", db_path.to_string_lossy());

    // Initialize AgentService in temporary test directory
    let service = AgentService::init(&db_url, &socket_path, 4)
        .await
        .expect("AgentService::init failed");
    let service_handle = tokio::spawn(service.run());

    // Connect IpcClient
    let client = IpcClient::connect(&socket_path)
        .await
        .expect("IpcClient::connect failed");
    let mut event_rx = client.subscribe();

    // 1. Status check: "agent.status" returns version "0.1.0" and active counts
    let status = client
        .call("agent.status", serde_json::json!({}))
        .await
        .expect("agent.status failed");
    assert_eq!(status["version"], "0.1.0");
    assert_eq!(status["active_tasks"], 0);
    assert_eq!(status["running_executions"], 0);
    assert!(status["uptime_secs"].is_number());

    // 2. Task CRUD: "task.save", "task.get", "task.list"
    let task_id = TaskId::new();
    let action_id = ActionId::new();
    let trigger_id = TriggerId::new();

    let task = Task {
        id: task_id,
        name: "IPC Daemon Job".to_string(),
        description: Some("End-to-end test job".to_string()),
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
                command: "echo 'daemon IPC live output 98765'".to_string(),
            },
        }],
        execution_policy: ExecutionPolicy::default(),
        working_directory: None,
        environment: HashMap::new(),
        version: 1,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    };

    // Save task
    let saved = client
        .call("task.save", serde_json::json!({ "task": task }))
        .await
        .expect("task.save failed");
    assert_eq!(saved["name"], "IPC Daemon Job");
    assert_eq!(saved["id"], task_id.to_string());

    // Get task
    let fetched = client
        .call("task.get", serde_json::json!({ "id": task_id }))
        .await
        .expect("task.get failed");
    assert_eq!(fetched["id"], task_id.to_string());
    assert_eq!(fetched["name"], "IPC Daemon Job");

    // List tasks
    let task_list = client
        .call("task.list", serde_json::json!({}))
        .await
        .expect("task.list failed");
    let task_arr = task_list.as_array().expect("expected task list array");
    assert_eq!(task_arr.len(), 1);
    assert_eq!(task_arr[0]["id"], task_id.to_string());

    // Verify status active_tasks incremented
    let updated_status = client
        .call("agent.status", serde_json::json!({}))
        .await
        .expect("agent.status failed");
    assert_eq!(updated_status["active_tasks"], 1);

    // 3. Execution flow: "task.trigger_now"
    let trigger_res = client
        .call("task.trigger_now", serde_json::json!({ "id": task_id }))
        .await
        .expect("task.trigger_now failed");
    assert_eq!(trigger_res, serde_json::json!(true));

    // 4. Real-time IPC events: receives "execution.started", "execution.output", and "execution.finished"
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
                    if content.contains("daemon IPC live output 98765") {
                        output_seen = true;
                    }
                }
                "execution.finished" => {
                    assert_eq!(event.data["task_id"], task_id.to_string());
                    assert_eq!(event.data["status"], "Succeeded");
                    assert_eq!(event.data["exit_code"], 0);
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

    // Task deletion
    let delete_res = client
        .call("task.delete", serde_json::json!({ "id": task_id }))
        .await
        .expect("task.delete failed");
    assert_eq!(delete_res, serde_json::json!(true));

    let task_list_after = client
        .call("task.list", serde_json::json!({}))
        .await
        .expect("task.list after delete failed");
    assert_eq!(task_list_after, serde_json::json!([]));

    // 5. Graceful shutdown: "agent.shutdown" terminates service cleanly
    let shutdown_res = client
        .call("agent.shutdown", serde_json::json!({}))
        .await
        .expect("agent.shutdown failed");
    assert_eq!(shutdown_res, serde_json::json!(true));

    let join_res = tokio::time::timeout(Duration::from_secs(5), service_handle).await;
    assert!(join_res.is_ok(), "Service did not shut down within timeout");
    assert!(join_res.unwrap().is_ok(), "Service task failed");
}

fn agent_binary_path() -> PathBuf {
    if let Ok(path) = std::env::var("CARGO_BIN_EXE_easyjob-agent") {
        PathBuf::from(path)
    } else {
        let mut path = std::env::current_exe().expect("current test exe path");
        path.pop();
        if path.ends_with("deps") {
            path.pop();
        }
        #[cfg(target_os = "windows")]
        path.push("easyjob-agent.exe");
        #[cfg(not(target_os = "windows"))]
        path.push("easyjob-agent");
        path
    }
}

#[test]
fn test_agent_binary_cli_help() {
    let binary_path = agent_binary_path();
    let output = Command::new(binary_path)
        .arg("--help")
        .output()
        .expect("Failed to run easyjob-agent --help");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("--data-dir"));
    assert!(stdout.contains("--daemon"));
    assert!(stdout.contains("--max-concurrent"));
}

#[tokio::test]
#[ignore = "skipping per user request: requires full OS process-level IPC permissions"]
async fn test_agent_binary_already_running_exits_cleanly() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("easyjob.db");
    #[cfg(target_os = "windows")]
    let socket_path = PathBuf::from(format!(
        r"\\.\pipe\easyjob-test-cli-{}",
        uuid::Uuid::new_v4()
    ));
    #[cfg(not(target_os = "windows"))]
    let socket_path = dir.path().join("easyjob.sock");
    let lock_path = dir.path().join("agent.lock");
    let db_url = format!("sqlite://{}?mode=rwc", db_path.to_string_lossy());

    // Start a primary instance in this directory
    let _lock = easyjob_agent::lock::SingleInstanceLock::acquire(&lock_path, &socket_path)
        .await
        .expect("Acquire primary lock");

    let service = AgentService::init(&db_url, &socket_path, 2)
        .await
        .expect("Init primary service");
    let service_handle = tokio::spawn(service.run());

    // Connect client to verify service is running
    let client = IpcClient::connect(&socket_path)
        .await
        .expect("Connect to primary service");
    let status = client
        .call("agent.status", serde_json::json!({}))
        .await
        .expect("Check primary status");
    assert_eq!(status["version"], "0.1.0");

    // Launch binary pointing to same data-dir; it should detect already running and exit 0
    let binary_path = agent_binary_path();
    let output = Command::new(binary_path)
        .arg("--data-dir")
        .arg(dir.path())
        .output()
        .expect("Failed to execute second agent binary instance");

    assert!(
        output.status.success(),
        "Second instance should exit cleanly with 0"
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("easyjob-agent is already running"),
        "Stdout should contain notice: {}",
        stdout
    );

    // Shutdown primary service cleanly
    let _ = client.call("agent.shutdown", serde_json::json!({})).await;
    let _ = tokio::time::timeout(Duration::from_secs(5), service_handle).await;
}
