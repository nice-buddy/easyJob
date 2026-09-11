use chrono::{NaiveTime, Utc, Weekday};
use easyjob_common::{ActionId, TaskId, TriggerId};
use easyjob_domain::{
    action::{Action, ActionKind},
    execution::{Execution, ExecutionStatus},
    policy::{ConcurrencyPolicy, ExecutionPolicy, MissedRunPolicy, RetryPolicy},
    task::Task,
    trigger::{Trigger, TriggerKind},
};
use std::collections::HashMap;

#[test]
fn test_task_creation_and_serialization() {
    let task_id = TaskId::new();
    let trigger = Trigger {
        id: TriggerId::new(),
        task_id,
        enabled: true,
        kind: TriggerKind::Daily {
            time: NaiveTime::from_hms_opt(8, 30, 0).unwrap(),
            timezone: "Asia/Shanghai".to_string(),
        },
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };
    let action = Action {
        id: ActionId::new(),
        task_id,
        sequence: 1,
        enabled: true,
        kind: ActionKind::ExecuteShell {
            command: "echo hello".to_string(),
        },
    };
    let task = Task {
        id: task_id,
        name: "Morning Backup".to_string(),
        description: Some("Daily automated job".to_string()),
        enabled: true,
        triggers: vec![trigger],
        actions: vec![action],
        execution_policy: ExecutionPolicy {
            concurrency_policy: ConcurrencyPolicy::SkipIfRunning,
            missed_run_policy: MissedRunPolicy::RunOnce,
            retry_policy: RetryPolicy {
                max_retries: 2,
                delay_secs: 5,
            },
            timeout_secs: Some(300),
        },
        working_directory: None,
        environment: HashMap::new(),
        version: 1,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };

    let serialized = serde_json::to_string(&task).unwrap();
    let deserialized: Task = serde_json::from_str(&serialized).unwrap();
    assert_eq!(task.id, deserialized.id);
    assert_eq!(task.name, deserialized.name);
    assert_eq!(deserialized.triggers.len(), 1);
    assert_eq!(deserialized.actions.len(), 1);
}

#[test]
fn test_execution_creation_and_defaults() {
    let task_id = TaskId::new();
    let trigger_id = Some(TriggerId::new());
    let scheduled_at = Some(Utc::now());

    let execution = Execution::new(task_id, trigger_id, scheduled_at);
    assert_eq!(execution.task_id, task_id);
    assert_eq!(execution.trigger_id, trigger_id);
    assert_eq!(execution.status, ExecutionStatus::Queued);
    assert_eq!(execution.scheduled_at, scheduled_at);
    assert!(execution.finished_at.is_none());
    assert!(execution.duration_ms.is_none());
    assert!(execution.exit_code.is_none());
    assert!(execution.error_message.is_none());

    let json = serde_json::to_string(&execution).unwrap();
    let deserialized: Execution = serde_json::from_str(&json).unwrap();
    assert_eq!(execution.id, deserialized.id);
    assert_eq!(execution.status, deserialized.status);
}

#[test]
fn test_policy_defaults() {
    let default_policy = ExecutionPolicy::default();
    assert_eq!(
        default_policy.concurrency_policy,
        ConcurrencyPolicy::SkipIfRunning
    );
    assert_eq!(default_policy.missed_run_policy, MissedRunPolicy::RunOnce);
    assert_eq!(default_policy.retry_policy.max_retries, 0);
    assert_eq!(default_policy.retry_policy.delay_secs, 0);
    assert_eq!(default_policy.timeout_secs, None);
}

#[test]
fn test_all_trigger_kinds_serialization() {
    let triggers = vec![
        TriggerKind::Once {
            fire_at: Utc::now(),
        },
        TriggerKind::Daily {
            time: NaiveTime::from_hms_opt(12, 0, 0).unwrap(),
            timezone: "UTC".to_string(),
        },
        TriggerKind::Weekly {
            days_of_week: vec![Weekday::Mon, Weekday::Fri],
            time: NaiveTime::from_hms_opt(9, 0, 0).unwrap(),
            timezone: "America/New_York".to_string(),
        },
        TriggerKind::Interval {
            interval_secs: 60,
            start_at: Some(Utc::now()),
        },
        TriggerKind::AgentStarted,
    ];

    for kind in triggers {
        let json = serde_json::to_string(&kind).unwrap();
        let deserialized: TriggerKind = serde_json::from_str(&json).unwrap();
        assert_eq!(kind, deserialized);
    }
}

#[test]
fn test_all_action_kinds_serialization() {
    let actions = vec![
        ActionKind::ExecuteProgram {
            program: "ls".to_string(),
            args: vec!["-la".to_string()],
        },
        ActionKind::ExecuteShell {
            command: "echo test".to_string(),
        },
        ActionKind::ExecutePowerShell {
            script: "Get-Process".to_string(),
            no_profile: true,
        },
        ActionKind::ExecuteCmd {
            command: "dir".to_string(),
        },
    ];

    for kind in actions {
        let json = serde_json::to_string(&kind).unwrap();
        let deserialized: ActionKind = serde_json::from_str(&json).unwrap();
        assert_eq!(kind, deserialized);
    }
}

#[test]
fn test_execution_statuses_serialization() {
    let statuses = vec![
        ExecutionStatus::Queued,
        ExecutionStatus::Running,
        ExecutionStatus::Succeeded,
        ExecutionStatus::Failed,
        ExecutionStatus::TimedOut,
        ExecutionStatus::Cancelled,
        ExecutionStatus::Skipped,
        ExecutionStatus::Interrupted,
    ];

    for status in statuses {
        let json = serde_json::to_string(&status).unwrap();
        let deserialized: ExecutionStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(status, deserialized);
    }
}
