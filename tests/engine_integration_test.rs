use chrono::Utc;
use easyjob_common::{ActionId, TaskId, TriggerId};
use easyjob_domain::action::{Action, ActionKind};
use easyjob_domain::execution::ExecutionStatus;
use easyjob_domain::policy::{
    ConcurrencyPolicy, ExecutionPolicy, MissedRunPolicy, RetryPolicy, TaskNotificationPolicy,
};
use easyjob_domain::task::Task;
use easyjob_domain::trigger::{Trigger, TriggerKind};
use easyjob_executor::manager::ExecutionManager;
use easyjob_executor::runner::ProcessRunner;
use easyjob_persistence::db::init_pool;
use easyjob_persistence::execution_repo::{ExecutionRepository, SqliteExecutionRepository};
use easyjob_persistence::task_repo::{SqliteTaskRepository, TaskRepository};
use easyjob_scheduler::scheduler::{Scheduler, SchedulerCommand, TriggerEvent};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

#[tokio::test]
async fn test_end_to_end_task_trigger_and_execute() {
    // 1. Initialize DB
    let pool = init_pool("sqlite::memory:").await.unwrap();
    let task_repo = SqliteTaskRepository::new(pool.clone());
    let exec_repo = SqliteExecutionRepository::new(pool.clone());

    // 2. Create Task
    let task_id = TaskId::new();
    let action_id = ActionId::new();
    let trigger_id = TriggerId::new();

    let task = Task {
        id: task_id,
        name: "E2E Test Task".to_string(),
        description: Some("Integration test runner".to_string()),
        enabled: true,
        triggers: vec![Trigger {
            id: trigger_id,
            task_id,
            enabled: true,
            kind: TriggerKind::Interval {
                interval_secs: 1,
                start_at: None,
            },
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }],
        actions: vec![Action {
            id: action_id,
            task_id,
            sequence: 1,
            enabled: true,
            kind: ActionKind::ExecuteShell {
                command: "echo 'E2E success'".to_string(),
            },
        }],
        execution_policy: ExecutionPolicy {
            concurrency_policy: ConcurrencyPolicy::AllowParallel,
            missed_run_policy: MissedRunPolicy::RunOnce,
            retry_policy: RetryPolicy::default(),
            timeout_secs: Some(5),
            notification: TaskNotificationPolicy::default(),
            log_retention: easyjob_domain::LogRetentionPolicy::SystemDefault,
        },
        working_directory: None,
        environment: HashMap::new(),
        version: 1,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };

    task_repo.save(&task).await.unwrap();

    // 3. Setup Scheduler & Execution Manager
    let (event_tx, mut event_rx) = mpsc::channel(10);
    let (scheduler, cmd_rx) = Scheduler::new(event_tx.clone());
    let exec_manager = Arc::new(ExecutionManager::new(4));

    // Spawn scheduler background loop to process commands and timers
    let scheduler_handle = tokio::spawn(Scheduler::run(
        scheduler.queue(),
        cmd_rx,
        scheduler.event_sender(),
    ));

    // Manually trigger task
    scheduler
        .sender()
        .send(SchedulerCommand::TriggerNow(task_id))
        .await
        .unwrap();

    // 4. Handle Trigger Event & Execute
    let event: TriggerEvent = event_rx.recv().await.unwrap();
    assert_eq!(event.task_id, task_id);

    // Concurrency slot acquisition
    let acquired = exec_manager
        .try_acquire_slot(&event.task_id, task.execution_policy.concurrency_policy)
        .await;
    assert!(
        acquired,
        "Failed to acquire execution slot for triggered task"
    );

    let loaded_task = task_repo.find_by_id(&event.task_id).await.unwrap().unwrap();
    let action = &loaded_task.actions[0];
    let cancel = CancellationToken::new();

    let started_at = Utc::now();
    let res = ProcessRunner::run_action(
        action,
        loaded_task.working_directory.as_ref(),
        &loaded_task.environment,
        loaded_task.execution_policy.timeout_secs,
        cancel,
    )
    .await
    .unwrap();

    exec_manager.release_slot(&event.task_id).await;

    assert_eq!(res.status, ExecutionStatus::Succeeded);
    assert!(res.stdout.contains("E2E success"));

    let finished_at = Utc::now();
    let duration_ms = (finished_at - started_at).num_milliseconds().max(0) as u64;

    // Record execution and output persistence
    let execution_id = easyjob_common::ExecutionId::new();
    let execution = easyjob_domain::execution::Execution {
        id: execution_id,
        task_id,
        trigger_id: event.trigger_id,
        status: res.status,
        scheduled_at: Some(event.scheduled_at),
        started_at,
        finished_at: Some(finished_at),
        duration_ms: Some(duration_ms),
        exit_code: res.exit_code,
        error_message: res.error_message,
    };
    exec_repo.create_run(&execution).await.unwrap();
    exec_repo
        .append_output(&execution_id, "stdout", &res.stdout)
        .await
        .unwrap();

    let saved_run = exec_repo
        .find_run_by_id(&execution_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(saved_run.status, ExecutionStatus::Succeeded);
    assert_eq!(saved_run.exit_code, Some(0));

    // Clean shutdown of scheduler
    scheduler
        .sender()
        .send(SchedulerCommand::Shutdown)
        .await
        .unwrap();
    scheduler_handle.await.unwrap();
}

#[tokio::test]
async fn test_end_to_end_scheduled_timer_trigger_and_execute() {
    // 1. Initialize DB
    let pool = init_pool("sqlite::memory:").await.unwrap();
    let task_repo = SqliteTaskRepository::new(pool.clone());
    let exec_repo = SqliteExecutionRepository::new(pool.clone());

    // 2. Create Task with Once trigger firing in 50ms
    let task_id = TaskId::new();
    let action_id = ActionId::new();
    let trigger_id = TriggerId::new();
    let fire_at = Utc::now() + chrono::Duration::milliseconds(50);

    let task = Task {
        id: task_id,
        name: "Scheduled E2E Task".to_string(),
        description: Some("Scheduled trigger integration test".to_string()),
        enabled: true,
        triggers: vec![Trigger {
            id: trigger_id,
            task_id,
            enabled: true,
            kind: TriggerKind::Once { fire_at },
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }],
        actions: vec![Action {
            id: action_id,
            task_id,
            sequence: 1,
            enabled: true,
            kind: ActionKind::ExecuteShell {
                command: "echo 'Scheduled E2E success'".to_string(),
            },
        }],
        execution_policy: ExecutionPolicy {
            concurrency_policy: ConcurrencyPolicy::SkipIfRunning,
            missed_run_policy: MissedRunPolicy::RunOnce,
            retry_policy: RetryPolicy::default(),
            timeout_secs: Some(5),
            notification: TaskNotificationPolicy::default(),
            log_retention: easyjob_domain::LogRetentionPolicy::SystemDefault,
        },
        working_directory: None,
        environment: HashMap::new(),
        version: 1,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };

    task_repo.save(&task).await.unwrap();

    // 3. Setup Scheduler & Execution Manager
    let (event_tx, mut event_rx) = mpsc::channel(10);
    let (scheduler, cmd_rx) = Scheduler::new(event_tx.clone());
    let exec_manager = Arc::new(ExecutionManager::new(4));

    let scheduler_handle = tokio::spawn(Scheduler::run(
        scheduler.queue(),
        cmd_rx,
        scheduler.event_sender(),
    ));

    // Register task with scheduler
    scheduler
        .sender()
        .send(SchedulerCommand::add_task(task.clone()))
        .await
        .unwrap();

    // 4. Wait for scheduled trigger event
    let event: TriggerEvent =
        tokio::time::timeout(std::time::Duration::from_millis(1500), event_rx.recv())
            .await
            .expect("timed out waiting for scheduled event")
            .expect("event received");

    assert_eq!(event.task_id, task_id);
    assert_eq!(event.trigger_id, Some(trigger_id));

    // Acquire slot with SkipIfRunning
    let acquired = exec_manager
        .try_acquire_slot(&event.task_id, task.execution_policy.concurrency_policy)
        .await;
    assert!(acquired);

    // Second slot request should fail under SkipIfRunning
    let second_acquired = exec_manager
        .try_acquire_slot(&event.task_id, task.execution_policy.concurrency_policy)
        .await;
    assert!(!second_acquired);

    let loaded_task = task_repo.find_by_id(&event.task_id).await.unwrap().unwrap();
    let action = &loaded_task.actions[0];
    let cancel = CancellationToken::new();

    let started_at = Utc::now();
    let res = ProcessRunner::run_action(
        action,
        loaded_task.working_directory.as_ref(),
        &loaded_task.environment,
        loaded_task.execution_policy.timeout_secs,
        cancel,
    )
    .await
    .unwrap();

    exec_manager.release_slot(&event.task_id).await;

    assert_eq!(res.status, ExecutionStatus::Succeeded);
    assert!(res.stdout.contains("Scheduled E2E success"));

    let finished_at = Utc::now();
    let duration_ms = (finished_at - started_at).num_milliseconds().max(0) as u64;

    let execution_id = easyjob_common::ExecutionId::new();
    let execution = easyjob_domain::execution::Execution {
        id: execution_id,
        task_id,
        trigger_id: event.trigger_id,
        status: res.status,
        scheduled_at: Some(event.scheduled_at),
        started_at,
        finished_at: Some(finished_at),
        duration_ms: Some(duration_ms),
        exit_code: res.exit_code,
        error_message: res.error_message,
    };
    exec_repo.create_run(&execution).await.unwrap();
    exec_repo
        .append_output(&execution_id, "stdout", &res.stdout)
        .await
        .unwrap();

    let saved_run = exec_repo
        .find_run_by_id(&execution_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(saved_run.status, ExecutionStatus::Succeeded);
    assert_eq!(saved_run.trigger_id, Some(trigger_id));
    assert_eq!(saved_run.exit_code, Some(0));

    // Clean shutdown of scheduler
    scheduler
        .sender()
        .send(SchedulerCommand::Shutdown)
        .await
        .unwrap();
    scheduler_handle.await.unwrap();
}
