use easyjob_common::{ActionId, TaskId};
use easyjob_domain::action::{Action, ActionKind};
use easyjob_domain::execution::ExecutionStatus;
use easyjob_domain::policy::ConcurrencyPolicy;
use easyjob_executor::manager::{ExecutionManager, ExecutionRequest};
use easyjob_executor::runner::{ProcessRunner, MAX_OUTPUT_BYTES};
use std::collections::HashMap;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

#[tokio::test]
async fn test_process_runner_success() {
    let action = Action {
        id: ActionId::new(),
        task_id: TaskId::new(),
        sequence: 1,
        enabled: true,
        kind: ActionKind::ExecuteShell {
            command: "echo 'running test'".to_string(),
        },
    };

    let cancel = CancellationToken::new();
    let res = ProcessRunner::run_action(&action, None, &Default::default(), Some(5), cancel)
        .await
        .unwrap();
    assert_eq!(res.status, ExecutionStatus::Succeeded);
    assert_eq!(res.exit_code, Some(0));
    assert!(res.stdout.contains("running test"));
    assert!(res.error_message.is_none());
}

#[tokio::test]
async fn test_process_runner_failing_action() {
    let action = Action {
        id: ActionId::new(),
        task_id: TaskId::new(),
        sequence: 1,
        enabled: true,
        kind: ActionKind::ExecuteShell {
            #[cfg(unix)]
            command: "exit 42".to_string(),
            #[cfg(windows)]
            command: "exit /b 42".to_string(),
        },
    };

    let cancel = CancellationToken::new();
    let res = ProcessRunner::run_action(&action, None, &Default::default(), Some(5), cancel)
        .await
        .unwrap();
    assert_eq!(res.status, ExecutionStatus::Failed);
    assert_eq!(res.exit_code, Some(42));
    assert!(res.error_message.is_some());
}

#[tokio::test]
async fn test_process_runner_timeout() {
    let action = Action {
        id: ActionId::new(),
        task_id: TaskId::new(),
        sequence: 1,
        enabled: true,
        kind: ActionKind::ExecuteShell {
            #[cfg(unix)]
            command: "sleep 10".to_string(),
            #[cfg(windows)]
            command: "ping -n 10 127.0.0.1 >nul".to_string(),
        },
    };

    let cancel = CancellationToken::new();
    let res = ProcessRunner::run_action(&action, None, &Default::default(), Some(1), cancel)
        .await
        .unwrap();
    assert_eq!(res.status, ExecutionStatus::TimedOut);
    assert_eq!(res.exit_code, None);
    assert!(res.error_message.unwrap().contains("Timed out"));
}

#[tokio::test]
async fn test_process_runner_cancellation() {
    let action = Action {
        id: ActionId::new(),
        task_id: TaskId::new(),
        sequence: 1,
        enabled: true,
        kind: ActionKind::ExecuteShell {
            #[cfg(unix)]
            command: "sleep 10".to_string(),
            #[cfg(windows)]
            command: "ping -n 10 127.0.0.1 >nul".to_string(),
        },
    };

    let cancel = CancellationToken::new();
    let cancel_clone = cancel.clone();

    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(200)).await;
        cancel_clone.cancel();
    });

    let res = ProcessRunner::run_action(&action, None, &Default::default(), Some(10), cancel)
        .await
        .unwrap();
    assert_eq!(res.status, ExecutionStatus::Cancelled);
    assert_eq!(res.exit_code, None);
    assert!(res.error_message.unwrap().contains("Cancelled"));
}

#[tokio::test]
async fn test_process_runner_cancellation_before_start() {
    let action = Action {
        id: ActionId::new(),
        task_id: TaskId::new(),
        sequence: 1,
        enabled: true,
        kind: ActionKind::ExecuteShell {
            command: "echo 'should not run'".to_string(),
        },
    };

    let cancel = CancellationToken::new();
    cancel.cancel(); // cancel immediately

    let res = ProcessRunner::run_action(&action, None, &Default::default(), Some(5), cancel)
        .await
        .unwrap();
    assert_eq!(res.status, ExecutionStatus::Cancelled);
    assert_eq!(res.exit_code, None);
}

#[tokio::test]
async fn test_process_runner_truncation_2mb() {
    // Generate ~3MB of output to trigger the 2MB limit
    let action = Action {
        id: ActionId::new(),
        task_id: TaskId::new(),
        sequence: 1,
        enabled: true,
        kind: ActionKind::ExecuteShell {
            #[cfg(unix)]
            command: "python3 -c \"print('A' * 3000000)\"".to_string(),
            #[cfg(windows)]
            command: "python -c \"print('A' * 3000000)\"".to_string(),
        },
    };

    let cancel = CancellationToken::new();
    let res = ProcessRunner::run_action(&action, None, &Default::default(), Some(10), cancel)
        .await
        .unwrap();

    assert_eq!(res.status, ExecutionStatus::Succeeded);
    assert!(res.stdout.contains("... [output truncated]"));
    // Capped at MAX_OUTPUT_BYTES + truncation message length
    assert!(res.stdout.len() <= MAX_OUTPUT_BYTES + 100);
}

#[tokio::test]
async fn test_process_runner_env_and_working_dir() {
    let temp_dir = std::env::temp_dir();
    let mut env = HashMap::new();
    env.insert(
        "EASYJOB_TEST_VAR".to_string(),
        "easyjob_value_123".to_string(),
    );

    let action = Action {
        id: ActionId::new(),
        task_id: TaskId::new(),
        sequence: 1,
        enabled: true,
        kind: ActionKind::ExecuteShell {
            #[cfg(unix)]
            command: "echo $EASYJOB_TEST_VAR && pwd".to_string(),
            #[cfg(windows)]
            command: "echo %EASYJOB_TEST_VAR% && cd".to_string(),
        },
    };

    let cancel = CancellationToken::new();
    let res = ProcessRunner::run_action(&action, Some(&temp_dir), &env, Some(5), cancel)
        .await
        .unwrap();

    assert_eq!(res.status, ExecutionStatus::Succeeded);
    assert!(res.stdout.contains("easyjob_value_123"));
}

#[tokio::test]
async fn test_execution_manager_skip_if_running() {
    let manager = ExecutionManager::new(4);
    let task_id = TaskId::new();

    let acquired_first = manager
        .try_acquire_slot(&task_id, ConcurrencyPolicy::SkipIfRunning)
        .await;
    assert!(acquired_first);

    let acquired_second = manager
        .try_acquire_slot(&task_id, ConcurrencyPolicy::SkipIfRunning)
        .await;
    assert!(!acquired_second); // Skipped!

    manager.release_slot(&task_id).await;
    let acquired_third = manager
        .try_acquire_slot(&task_id, ConcurrencyPolicy::SkipIfRunning)
        .await;
    assert!(acquired_third);
}

#[tokio::test]
async fn test_execution_manager_allow_parallel() {
    let manager = ExecutionManager::new(4);
    let task_id = TaskId::new();

    assert!(
        manager
            .try_acquire_slot(&task_id, ConcurrencyPolicy::AllowParallel)
            .await
    );
    assert!(
        manager
            .try_acquire_slot(&task_id, ConcurrencyPolicy::AllowParallel)
            .await
    );
    assert!(
        manager
            .try_acquire_slot(&task_id, ConcurrencyPolicy::AllowParallel)
            .await
    );

    manager.release_slot(&task_id).await;
    manager.release_slot(&task_id).await;
    manager.release_slot(&task_id).await;
}

#[tokio::test]
async fn test_execution_manager_queue_one() {
    let manager = ExecutionManager::new(4);
    let task_id = TaskId::new();

    // 1st run (running)
    assert!(
        manager
            .try_acquire_slot(&task_id, ConcurrencyPolicy::QueueOne)
            .await
    );
    // 2nd run (queued)
    assert!(
        manager
            .try_acquire_slot(&task_id, ConcurrencyPolicy::QueueOne)
            .await
    );
    // 3rd run rejected
    assert!(
        !manager
            .try_acquire_slot(&task_id, ConcurrencyPolicy::QueueOne)
            .await
    );

    manager.release_slot(&task_id).await;
    // Now one slot freed, 3rd can be acquired
    assert!(
        manager
            .try_acquire_slot(&task_id, ConcurrencyPolicy::QueueOne)
            .await
    );
}

#[tokio::test]
async fn test_execution_manager_global_semaphore() {
    let manager = ExecutionManager::new(2);
    let sem = manager.global_semaphore();

    let permit1 = sem.clone().try_acquire_owned().unwrap();
    let permit2 = sem.clone().try_acquire_owned().unwrap();
    assert!(sem.clone().try_acquire_owned().is_err());

    drop(permit1);
    let permit3 = sem.clone().try_acquire_owned().unwrap();
    drop(permit2);
    drop(permit3);
}

#[tokio::test]
async fn test_execution_request() {
    let manager = ExecutionManager::new(4);
    let task_id = TaskId::new();
    let req = ExecutionRequest::new(task_id, ConcurrencyPolicy::SkipIfRunning);

    assert!(manager.try_acquire_request(&req).await);
    assert!(!manager.try_acquire_request(&req).await);

    manager.release_slot(&task_id).await;
    assert!(manager.try_acquire_request(&req).await);
}

#[tokio::test]
async fn test_process_runner_execute_program() {
    #[cfg(unix)]
    let (program, args) = ("echo".to_string(), vec!["program test arg".to_string()]);
    #[cfg(windows)]
    let (program, args) = (
        "cmd.exe".to_string(),
        vec!["/c".to_string(), "echo program test arg".to_string()],
    );

    let action = Action {
        id: ActionId::new(),
        task_id: TaskId::new(),
        sequence: 1,
        enabled: true,
        kind: ActionKind::ExecuteProgram { program, args },
    };

    let cancel = CancellationToken::new();
    let res = ProcessRunner::run_action(&action, None, &Default::default(), Some(5), cancel)
        .await
        .unwrap();

    assert_eq!(res.status, ExecutionStatus::Succeeded);
    assert_eq!(res.exit_code, Some(0));
    assert!(res.stdout.contains("program test arg"));
}
