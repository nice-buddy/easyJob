use easyjob_agent::network::{dispatch_network_event, NetworkEvent};
use easyjob_common::{ActionId, TaskId, TriggerId};
use easyjob_domain::action::{Action, ActionKind};
use easyjob_domain::policy::ExecutionPolicy;
use easyjob_domain::task::Task;
use easyjob_domain::trigger::{NetworkEventKind, Trigger, TriggerKind};
use easyjob_persistence::task_repo::{SqliteTaskRepository, TaskRepository};
use easyjob_scheduler::scheduler::{Scheduler, SchedulerCommand, TriggerEvent};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tempfile::TempDir;
use tokio::sync::mpsc;

/// 集成测试夹具：真实 SQLite 文件库 + 真实 Scheduler 循环。
/// `TempDir` 必须随夹具返回，否则目录在测试期间被删。
async fn setup() -> (
    Arc<SqliteTaskRepository>,
    mpsc::Sender<SchedulerCommand>,
    mpsc::Receiver<TriggerEvent>,
    TempDir,
    tokio::task::JoinHandle<()>,
) {
    let dir = tempfile::tempdir().unwrap();
    let db_url = format!("sqlite://{}/test.db?mode=rwc", dir.path().display());
    let pool = easyjob_persistence::init_pool(&db_url).await.unwrap();
    let repo = Arc::new(SqliteTaskRepository::new(pool));

    let (event_tx, event_rx) = mpsc::channel::<TriggerEvent>(16);
    let (scheduler, cmd_rx) = Scheduler::new(event_tx);
    let handle = tokio::spawn(Scheduler::run(
        scheduler.queue(),
        cmd_rx,
        scheduler.event_sender(),
    ));

    (repo, scheduler.sender(), event_rx, dir, handle)
}

fn task_with_network_trigger(
    enabled: bool,
    trigger_enabled: bool,
    events: Vec<NetworkEventKind>,
    network_name: Option<String>,
) -> Task {
    let task_id = TaskId::new();
    Task {
        id: task_id,
        name: "network integration test".to_string(),
        description: None,
        enabled,
        triggers: vec![Trigger {
            id: TriggerId::new(),
            task_id,
            enabled: trigger_enabled,
            kind: TriggerKind::Network {
                events,
                network_name,
            },
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        }],
        actions: vec![Action {
            id: ActionId::new(),
            task_id,
            sequence: 1,
            enabled: true,
            kind: ActionKind::ExecuteShell {
                command: "true".to_string(),
            },
        }],
        execution_policy: ExecutionPolicy::default(),
        working_directory: None,
        environment: HashMap::new(),
        version: 1,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    }
}

fn connect(ssid: Option<&str>) -> NetworkEvent {
    NetworkEvent::Connect {
        ssid: ssid.map(String::from),
    }
}

#[tokio::test]
async fn network_connect_event_triggers_matching_task() {
    let (repo, cmd_tx, mut event_rx, _dir, handle) = setup().await;

    let task = task_with_network_trigger(true, true, vec![NetworkEventKind::Connect], None);
    let expected_id = task.id;
    repo.save(&task).await.unwrap();

    dispatch_network_event(connect(Some("Home")), repo.clone(), cmd_tx.clone()).await;

    let ev = tokio::time::timeout(Duration::from_millis(500), event_rx.recv())
        .await
        .expect("expected a trigger event within timeout")
        .expect("trigger event channel closed");
    assert_eq!(ev.task_id, expected_id);
    assert_eq!(ev.trigger_id, None);

    handle.abort();
}

#[tokio::test]
async fn network_disconnect_event_skips_task_without_disconnect() {
    let (repo, cmd_tx, mut event_rx, _dir, handle) = setup().await;

    let task = task_with_network_trigger(true, true, vec![NetworkEventKind::Connect], None);
    repo.save(&task).await.unwrap();

    dispatch_network_event(
        NetworkEvent::Disconnect {
            ssid: Some("Home".to_string()),
        },
        repo.clone(),
        cmd_tx.clone(),
    )
    .await;

    let res = tokio::time::timeout(Duration::from_millis(300), event_rx.recv()).await;
    assert!(
        res.is_err(),
        "task not listening for Disconnect must not fire"
    );

    handle.abort();
}

#[tokio::test]
async fn network_ssid_filter_triggers_only_on_match() {
    let (repo, cmd_tx, mut event_rx, _dir, handle) = setup().await;

    let task = task_with_network_trigger(
        true,
        true,
        vec![NetworkEventKind::Connect],
        Some("Office".to_string()),
    );
    let expected_id = task.id;
    repo.save(&task).await.unwrap();

    // SSID 不匹配 → 不触发
    dispatch_network_event(connect(Some("Home")), repo.clone(), cmd_tx.clone()).await;
    let res = tokio::time::timeout(Duration::from_millis(300), event_rx.recv()).await;
    assert!(res.is_err(), "SSID mismatch must not fire");

    // SSID 匹配 → 触发
    dispatch_network_event(connect(Some("Office")), repo.clone(), cmd_tx.clone()).await;
    let ev = tokio::time::timeout(Duration::from_millis(500), event_rx.recv())
        .await
        .expect("expected a trigger event for matching SSID")
        .expect("trigger event channel closed");
    assert_eq!(ev.task_id, expected_id);
    assert_eq!(ev.trigger_id, None);

    handle.abort();
}

#[tokio::test]
async fn network_event_for_disabled_task_does_not_fire() {
    let (repo, cmd_tx, mut event_rx, _dir, handle) = setup().await;

    let task = task_with_network_trigger(false, true, vec![NetworkEventKind::Connect], None);
    repo.save(&task).await.unwrap();

    dispatch_network_event(connect(Some("Home")), repo.clone(), cmd_tx.clone()).await;

    let res = tokio::time::timeout(Duration::from_millis(300), event_rx.recv()).await;
    assert!(res.is_err(), "disabled task must not fire");

    handle.abort();
}
