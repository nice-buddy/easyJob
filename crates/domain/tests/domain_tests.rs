use chrono::{NaiveTime, Utc, Weekday};
use easyjob_common::{ActionId, TaskId, TriggerId};
use easyjob_domain::{
    action::{Action, ActionKind},
    execution::{Execution, ExecutionStatus},
    policy::{
        ConcurrencyPolicy, ExecutionPolicy, LogRetentionPolicy, MissedRunPolicy, RetryPolicy,
        TaskNotificationPolicy,
    },
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
            notification: TaskNotificationPolicy::None,
            log_retention: LogRetentionPolicy::SystemDefault,
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
    assert_eq!(default_policy.notification, TaskNotificationPolicy::None);
    assert_eq!(
        default_policy.log_retention,
        LogRetentionPolicy::SystemDefault
    );
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

#[test]
fn test_task_notification_policy_serialization_and_backward_compatibility() {
    use easyjob_domain::policy::TaskNotificationPolicy;

    // 1. 验证新结构包含 notification 时的完整往返序列化
    let policy = ExecutionPolicy {
        concurrency_policy: ConcurrencyPolicy::SkipIfRunning,
        missed_run_policy: MissedRunPolicy::RunOnce,
        retry_policy: RetryPolicy::default(),
        timeout_secs: Some(60),
        notification: TaskNotificationPolicy::OnlyFailure,
        log_retention: LogRetentionPolicy::SystemDefault,
    };

    let json_str = serde_json::to_string(&policy).unwrap();
    assert!(json_str.contains("\"notification\":\"OnlyFailure\""));
    let deserialized: ExecutionPolicy = serde_json::from_str(&json_str).unwrap();
    assert_eq!(
        deserialized.notification,
        TaskNotificationPolicy::OnlyFailure
    );

    // 2. 验证向后兼容性：旧版本 JSON 没有 notification 字段时，必须默认反序列化为 TaskNotificationPolicy::None
    let legacy_json = r#"{
        "concurrency_policy": "AllowParallel",
        "missed_run_policy": "Skip",
        "retry_policy": {"max_retries": 1, "delay_secs": 2},
        "timeout_secs": 120
    }"#;
    let legacy_deserialized: ExecutionPolicy = serde_json::from_str(legacy_json).unwrap();
    assert_eq!(
        legacy_deserialized.notification,
        TaskNotificationPolicy::None
    );
}

#[test]
fn test_log_retention_policy_defaults_and_serde() {
    use easyjob_domain::policy::{
        ExecutionPolicy, LogRetentionPolicy, SystemLogRetention, SystemSettings,
    };

    // 1. 验证默认策略为 SystemDefault
    let default_policy = LogRetentionPolicy::default();
    assert_eq!(default_policy, LogRetentionPolicy::SystemDefault);

    // 2. 验证序列化与反序列化
    let keep_7 = LogRetentionPolicy::KeepDays(7);
    let json_keep_7 = serde_json::to_string(&keep_7).unwrap();
    assert_eq!(json_keep_7, r#"{"mode":"KeepDays","days":7}"#);
    let parsed_keep_7: LogRetentionPolicy = serde_json::from_str(&json_keep_7).unwrap();
    assert_eq!(parsed_keep_7, keep_7);

    let perm = LogRetentionPolicy::Permanent;
    let json_perm = serde_json::to_string(&perm).unwrap();
    assert_eq!(json_perm, r#"{"mode":"Permanent"}"#);
    let parsed_perm: LogRetentionPolicy = serde_json::from_str(&json_perm).unwrap();
    assert_eq!(parsed_perm, perm);

    // 3. 验证旧版 ExecutionPolicy JSON 缺失 log_retention 时的向后兼容性
    let legacy_json = r#"{
        "concurrency_policy": "SkipIfRunning",
        "missed_run_policy": "RunOnce",
        "retry_policy": { "max_retries": 0, "delay_secs": 0 },
        "timeout_secs": 3600,
        "notification": "None"
    }"#;
    let ep: ExecutionPolicy = serde_json::from_str(legacy_json).unwrap();
    assert_eq!(ep.log_retention, LogRetentionPolicy::SystemDefault);

    // 4. 验证 SystemSettings 默认值为 KeepDays(7)
    let sys_default = SystemSettings::default();
    assert_eq!(
        sys_default.default_log_retention,
        SystemLogRetention::KeepDays(7)
    );
    let sys_json = serde_json::to_string(&sys_default).unwrap();
    let parsed_sys: SystemSettings = serde_json::from_str(&sys_json).unwrap();
    assert_eq!(parsed_sys, sys_default);
}
