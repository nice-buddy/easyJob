use chrono::Utc;
use easyjob_common::{ActionId, TaskId, TriggerId};
use easyjob_domain::action::{Action, ActionKind};
use easyjob_domain::execution::{Execution, ExecutionStatus};
use easyjob_domain::policy::ExecutionPolicy;
use easyjob_domain::task::Task;
use easyjob_domain::trigger::{Trigger, TriggerKind};
use easyjob_domain::{SystemLogRetention, SystemSettings};
use easyjob_persistence::db::init_pool;
use easyjob_persistence::execution_repo::{ExecutionRepository, SqliteExecutionRepository};
use easyjob_persistence::recovery::recover_dangling_executions;
use easyjob_persistence::task_repo::{SqliteTaskRepository, TaskRepository};
use easyjob_persistence::{SettingsRepository, SqliteSettingsRepository};
use sqlx::Row;
use std::collections::HashMap;

#[tokio::test]
async fn test_task_save_load_and_recovery() {
    let pool = init_pool("sqlite::memory:?cache=shared").await.unwrap();
    let task_repo = SqliteTaskRepository::new(pool.clone());
    let exec_repo = SqliteExecutionRepository::new(pool.clone());

    let task_id = TaskId::new();
    let task = Task {
        id: task_id,
        name: "Repo Test Task".to_string(),
        description: None,
        enabled: true,
        triggers: vec![Trigger {
            id: TriggerId::new(),
            task_id,
            enabled: true,
            kind: TriggerKind::AgentStarted,
            created_at: Utc::now(),
            updated_at: Utc::now(),
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
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };

    task_repo.save(&task).await.unwrap();
    let loaded = task_repo
        .find_by_id(&task_id)
        .await
        .unwrap()
        .expect("task found");
    assert_eq!(loaded.name, "Repo Test Task");
    assert_eq!(loaded.triggers.len(), 1);
    assert_eq!(loaded.actions.len(), 1);

    // Test execution dangling recovery
    let mut dangling_exec = Execution::new(task_id, None, None);
    dangling_exec.status = ExecutionStatus::Running;
    exec_repo.create_run(&dangling_exec).await.unwrap();

    let recovered = recover_dangling_executions(&pool).await.unwrap();
    assert_eq!(recovered, 1);

    let updated_exec = exec_repo
        .find_run_by_id(&dangling_exec.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(updated_exec.status, ExecutionStatus::Interrupted);
    assert_eq!(
        updated_exec.error_message.as_deref(),
        Some("Agent restarted unexpectedly")
    );
}

#[tokio::test]
async fn test_task_update_and_version_bump() {
    let pool = init_pool("sqlite::memory:?cache=shared").await.unwrap();
    let task_repo = SqliteTaskRepository::new(pool.clone());

    let task_id = TaskId::new();
    let mut task = Task {
        id: task_id,
        name: "Initial Name".to_string(),
        description: Some("Initial Description".to_string()),
        enabled: true,
        triggers: vec![Trigger {
            id: TriggerId::new(),
            task_id,
            enabled: true,
            kind: TriggerKind::AgentStarted,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }],
        actions: vec![Action {
            id: ActionId::new(),
            task_id,
            sequence: 1,
            enabled: true,
            kind: ActionKind::ExecuteShell {
                command: "echo 1".to_string(),
            },
        }],
        execution_policy: ExecutionPolicy::default(),
        working_directory: None,
        environment: HashMap::new(),
        version: 1,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };

    task_repo.save(&task).await.unwrap();

    let loaded = task_repo.find_by_id(&task_id).await.unwrap().unwrap();
    assert_eq!(loaded.name, "Initial Name");
    assert_eq!(loaded.version, 1);

    // Update task
    task.name = "Updated Name".to_string();
    task.actions.push(Action {
        id: ActionId::new(),
        task_id,
        sequence: 2,
        enabled: true,
        kind: ActionKind::ExecuteShell {
            command: "echo 2".to_string(),
        },
    });
    task_repo.save(&task).await.unwrap();

    let updated = task_repo.find_by_id(&task_id).await.unwrap().unwrap();
    assert_eq!(updated.name, "Updated Name");
    assert_eq!(updated.version, 2);
    assert_eq!(updated.actions.len(), 2);
}

#[tokio::test]
async fn test_task_find_all_enabled_and_cascade_delete() {
    let pool = init_pool("sqlite::memory:?cache=shared").await.unwrap();
    let task_repo = SqliteTaskRepository::new(pool.clone());

    let task_id_enabled = TaskId::new();
    let task_enabled = Task {
        id: task_id_enabled,
        name: "Enabled Task".to_string(),
        description: None,
        enabled: true,
        triggers: vec![],
        actions: vec![Action {
            id: ActionId::new(),
            task_id: task_id_enabled,
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
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };

    let task_id_disabled = TaskId::new();
    let task_disabled = Task {
        id: task_id_disabled,
        name: "Disabled Task".to_string(),
        description: None,
        enabled: false,
        triggers: vec![],
        actions: vec![],
        execution_policy: ExecutionPolicy::default(),
        working_directory: None,
        environment: HashMap::new(),
        version: 1,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };

    task_repo.save(&task_enabled).await.unwrap();
    task_repo.save(&task_disabled).await.unwrap();

    let all_tasks = task_repo.find_all().await.unwrap();
    assert!(all_tasks.iter().any(|t| t.id == task_id_enabled));
    assert!(all_tasks.iter().any(|t| t.id == task_id_disabled));

    let enabled_tasks = task_repo.find_all_enabled().await.unwrap();
    assert!(enabled_tasks.iter().any(|t| t.id == task_id_enabled));
    assert!(!enabled_tasks.iter().any(|t| t.id == task_id_disabled));

    // Delete enabled task and verify cascading
    task_repo.delete(&task_id_enabled).await.unwrap();
    assert!(task_repo
        .find_by_id(&task_id_enabled)
        .await
        .unwrap()
        .is_none());

    // Verify actions table cascaded
    let action_count: (i64,) = sqlx::query_as("SELECT count(*) FROM actions WHERE task_id = ?")
        .bind(task_id_enabled.to_string())
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(action_count.0, 0);
}

#[tokio::test]
async fn test_execution_repo_crud_and_append_output() {
    let pool = init_pool("sqlite::memory:?cache=shared").await.unwrap();
    let task_repo = SqliteTaskRepository::new(pool.clone());
    let exec_repo = SqliteExecutionRepository::new(pool.clone());

    let task_id = TaskId::new();
    let task = Task {
        id: task_id,
        name: "Exec Test Task".to_string(),
        description: None,
        enabled: true,
        triggers: vec![],
        actions: vec![],
        execution_policy: ExecutionPolicy::default(),
        working_directory: None,
        environment: HashMap::new(),
        version: 1,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };
    task_repo.save(&task).await.unwrap();

    let mut exec = Execution::new(task_id, None, Some(Utc::now()));
    exec.status = ExecutionStatus::Running;
    exec_repo.create_run(&exec).await.unwrap();

    let found = exec_repo
        .find_run_by_id(&exec.id)
        .await
        .unwrap()
        .expect("run found");
    assert_eq!(found.status, ExecutionStatus::Running);
    assert_eq!(found.task_id, task_id);
    assert!(found.scheduled_at.is_some());

    // Update run
    exec.status = ExecutionStatus::Succeeded;
    exec.finished_at = Some(Utc::now());
    exec.duration_ms = Some(150);
    exec.exit_code = Some(0);
    exec_repo.update_run(&exec).await.unwrap();

    let updated = exec_repo
        .find_run_by_id(&exec.id)
        .await
        .unwrap()
        .expect("run found");
    assert_eq!(updated.status, ExecutionStatus::Succeeded);
    assert_eq!(updated.duration_ms, Some(150));
    assert_eq!(updated.exit_code, Some(0));
    assert!(updated.finished_at.is_some());

    // Append outputs
    exec_repo
        .append_output(&exec.id, "stdout", "line 1\n")
        .await
        .unwrap();
    exec_repo
        .append_output(&exec.id, "stderr", "warning 1\n")
        .await
        .unwrap();

    let rows =
        sqlx::query("SELECT stream, content FROM run_outputs WHERE run_id = ? ORDER BY id ASC")
            .bind(exec.id.to_string())
            .fetch_all(&pool)
            .await
            .unwrap();
    assert_eq!(rows.len(), 2);
    let s1: String = rows[0].get("stream");
    let c1: String = rows[0].get("content");
    assert_eq!(s1, "stdout");
    assert_eq!(c1, "line 1\n");

    let outputs = exec_repo.get_outputs(&exec.id).await.unwrap();
    assert_eq!(outputs.len(), 2);
    assert_eq!(outputs[0].stream, "stdout");
    assert_eq!(outputs[0].content, "line 1\n");
    assert_eq!(outputs[1].stream, "stderr");
    assert_eq!(outputs[1].content, "warning 1\n");

    // Test find_recent_runs
    let mut exec2 = Execution::new(task_id, None, Some(Utc::now()));
    exec2.status = ExecutionStatus::Running;
    exec2.started_at = Utc::now() + chrono::Duration::seconds(10);
    exec_repo.create_run(&exec2).await.unwrap();

    let recent = exec_repo.find_recent_runs(10).await.unwrap();
    assert_eq!(recent.len(), 2);
    assert_eq!(recent[0].id, exec2.id);
    assert_eq!(recent[1].id, exec.id);

    let limited = exec_repo.find_recent_runs(1).await.unwrap();
    assert_eq!(limited.len(), 1);
    assert_eq!(limited[0].id, exec2.id);
    let s2: String = rows[1].get("stream");
    let c2: String = rows[1].get("content");
    assert_eq!(s2, "stderr");
    assert_eq!(c2, "warning 1\n");
}

#[tokio::test]
async fn test_recovery_selective_on_running_only() {
    let pool = init_pool("sqlite::memory:?cache=shared").await.unwrap();
    let task_repo = SqliteTaskRepository::new(pool.clone());
    let exec_repo = SqliteExecutionRepository::new(pool.clone());

    let task_id = TaskId::new();
    let task = Task {
        id: task_id,
        name: "Recovery Test Task".to_string(),
        description: None,
        enabled: true,
        triggers: vec![],
        actions: vec![],
        execution_policy: ExecutionPolicy::default(),
        working_directory: None,
        environment: HashMap::new(),
        version: 1,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };
    task_repo.save(&task).await.unwrap();

    // Create 1 Queued, 2 Running, 1 Succeeded, 1 Failed
    let mut exec_queued = Execution::new(task_id, None, None);
    exec_queued.status = ExecutionStatus::Queued;
    exec_repo.create_run(&exec_queued).await.unwrap();

    let mut exec_running1 = Execution::new(task_id, None, None);
    exec_running1.status = ExecutionStatus::Running;
    exec_repo.create_run(&exec_running1).await.unwrap();

    let mut exec_running2 = Execution::new(task_id, None, None);
    exec_running2.status = ExecutionStatus::Running;
    exec_repo.create_run(&exec_running2).await.unwrap();

    let mut exec_succeeded = Execution::new(task_id, None, None);
    exec_succeeded.status = ExecutionStatus::Succeeded;
    exec_repo.create_run(&exec_succeeded).await.unwrap();

    let mut exec_failed = Execution::new(task_id, None, None);
    exec_failed.status = ExecutionStatus::Failed;
    exec_repo.create_run(&exec_failed).await.unwrap();

    let recovered_count = recover_dangling_executions(&pool).await.unwrap();
    assert_eq!(recovered_count, 2);

    // Verify running runs became Interrupted
    let r1 = exec_repo
        .find_run_by_id(&exec_running1.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(r1.status, ExecutionStatus::Interrupted);
    assert_eq!(
        r1.error_message.as_deref(),
        Some("Agent restarted unexpectedly")
    );

    let r2 = exec_repo
        .find_run_by_id(&exec_running2.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(r2.status, ExecutionStatus::Interrupted);
    assert_eq!(
        r2.error_message.as_deref(),
        Some("Agent restarted unexpectedly")
    );

    // Verify others remained intact
    let q = exec_repo
        .find_run_by_id(&exec_queued.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(q.status, ExecutionStatus::Queued);

    let s = exec_repo
        .find_run_by_id(&exec_succeeded.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(s.status, ExecutionStatus::Succeeded);

    let f = exec_repo
        .find_run_by_id(&exec_failed.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(f.status, ExecutionStatus::Failed);
}

#[tokio::test]
async fn test_file_db_persistence_and_wal() {
    let temp_dir = std::env::temp_dir();
    let db_path = temp_dir.join(format!("easyjob_repo_test_{}.db", uuid::Uuid::new_v4()));
    let db_url = format!("sqlite://{}", db_path.to_string_lossy());

    let pool = init_pool(&db_url).await.unwrap();
    let task_repo = SqliteTaskRepository::new(pool.clone());
    let exec_repo = SqliteExecutionRepository::new(pool.clone());

    let task_id = TaskId::new();
    let task = Task {
        id: task_id,
        name: "File DB Task".to_string(),
        description: Some("Stored in disk file".to_string()),
        enabled: true,
        triggers: vec![Trigger {
            id: TriggerId::new(),
            task_id,
            enabled: true,
            kind: TriggerKind::AgentStarted,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }],
        actions: vec![Action {
            id: ActionId::new(),
            task_id,
            sequence: 1,
            enabled: true,
            kind: ActionKind::ExecuteShell {
                command: "echo file".to_string(),
            },
        }],
        execution_policy: ExecutionPolicy::default(),
        working_directory: None,
        environment: HashMap::new(),
        version: 1,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };

    task_repo.save(&task).await.unwrap();
    let loaded = task_repo.find_by_id(&task_id).await.unwrap().unwrap();
    assert_eq!(loaded.name, "File DB Task");

    let mut exec = Execution::new(task_id, None, None);
    exec.status = ExecutionStatus::Running;
    exec_repo.create_run(&exec).await.unwrap();

    let recovered = recover_dangling_executions(&pool).await.unwrap();
    assert_eq!(recovered, 1);

    let updated_exec = exec_repo.find_run_by_id(&exec.id).await.unwrap().unwrap();
    assert_eq!(updated_exec.status, ExecutionStatus::Interrupted);

    // Clean up
    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(format!("{}-wal", db_path.to_string_lossy()));
    let _ = std::fs::remove_file(format!("{}-shm", db_path.to_string_lossy()));
}

#[tokio::test]
async fn test_task_preserves_disabled_triggers_actions_and_increments_version() {
    let pool = init_pool("sqlite::memory:?cache=shared").await.unwrap();
    let task_repo = SqliteTaskRepository::new(pool.clone());

    let task_id = TaskId::new();
    let trigger_enabled_id = TriggerId::new();
    let trigger_disabled_id = TriggerId::new();
    let action_enabled_id = ActionId::new();
    let action_disabled_id = ActionId::new();

    let task = Task {
        id: task_id,
        name: "Disabled Items Task".to_string(),
        description: Some("Testing disabled triggers and actions".to_string()),
        enabled: true,
        triggers: vec![
            Trigger {
                id: trigger_enabled_id,
                task_id,
                enabled: true,
                kind: TriggerKind::AgentStarted,
                created_at: Utc::now(),
                updated_at: Utc::now(),
            },
            Trigger {
                id: trigger_disabled_id,
                task_id,
                enabled: false,
                kind: TriggerKind::Once {
                    fire_at: Utc::now(),
                },
                created_at: Utc::now(),
                updated_at: Utc::now(),
            },
        ],
        actions: vec![
            Action {
                id: action_enabled_id,
                task_id,
                sequence: 1,
                enabled: true,
                kind: ActionKind::ExecuteShell {
                    command: "echo enabled".to_string(),
                },
            },
            Action {
                id: action_disabled_id,
                task_id,
                sequence: 2,
                enabled: false,
                kind: ActionKind::ExecuteShell {
                    command: "echo disabled".to_string(),
                },
            },
        ],
        execution_policy: ExecutionPolicy::default(),
        working_directory: None,
        environment: HashMap::new(),
        version: 1,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };

    // Initial save
    task_repo.save(&task).await.unwrap();

    let loaded = task_repo
        .find_by_id(&task_id)
        .await
        .unwrap()
        .expect("task found");
    assert_eq!(loaded.version, 1);
    assert_eq!(loaded.triggers.len(), 2);
    assert_eq!(loaded.actions.len(), 2);

    let tr_enabled = loaded
        .triggers
        .iter()
        .find(|t| t.id == trigger_enabled_id)
        .unwrap();
    assert!(tr_enabled.enabled);

    let tr_disabled = loaded
        .triggers
        .iter()
        .find(|t| t.id == trigger_disabled_id)
        .unwrap();
    assert!(!tr_disabled.enabled);

    let act_enabled = loaded
        .actions
        .iter()
        .find(|a| a.id == action_enabled_id)
        .unwrap();
    assert!(act_enabled.enabled);
    assert_eq!(act_enabled.sequence, 1);

    let act_disabled = loaded
        .actions
        .iter()
        .find(|a| a.id == action_disabled_id)
        .unwrap();
    assert!(!act_disabled.enabled);
    assert_eq!(act_disabled.sequence, 2);

    // Repeated save 1 -> version increments to 2
    task_repo.save(&loaded).await.unwrap();
    let loaded_v2 = task_repo
        .find_by_id(&task_id)
        .await
        .unwrap()
        .expect("task found");
    assert_eq!(loaded_v2.version, 2);
    assert_eq!(loaded_v2.triggers.len(), 2);
    assert_eq!(loaded_v2.actions.len(), 2);

    // Repeated save 2 -> version increments to 3
    task_repo.save(&loaded_v2).await.unwrap();
    let loaded_v3 = task_repo
        .find_by_id(&task_id)
        .await
        .unwrap()
        .expect("task found");
    assert_eq!(loaded_v3.version, 3);
    assert_eq!(loaded_v3.triggers.len(), 2);
    assert_eq!(loaded_v3.actions.len(), 2);

    let tr_disabled_v3 = loaded_v3
        .triggers
        .iter()
        .find(|t| t.id == trigger_disabled_id)
        .unwrap();
    assert!(!tr_disabled_v3.enabled);

    let act_disabled_v3 = loaded_v3
        .actions
        .iter()
        .find(|a| a.id == action_disabled_id)
        .unwrap();
    assert!(!act_disabled_v3.enabled);
}

#[tokio::test]
async fn test_find_by_id_propagates_deserialization_errors() {
    let pool = init_pool("sqlite::memory:?cache=shared").await.unwrap();
    let task_repo = SqliteTaskRepository::new(pool.clone());
    let task_id = TaskId::new();

    sqlx::query(
        "INSERT INTO tasks (id, name, description, enabled, execution_policy_json, working_directory, environment_json, version, created_at, updated_at)
         VALUES (?, 'Corrupt Task', NULL, 1, '{}', NULL, '{}', 1, datetime('now'), datetime('now'))"
    )
    .bind(task_id.to_string())
    .execute(&pool)
    .await
    .unwrap();

    sqlx::query(
        "INSERT INTO triggers (id, task_id, kind, config_json, enabled, created_at, updated_at)
         VALUES (?, ?, 'AgentStarted', 'invalid json', 1, datetime('now'), datetime('now'))",
    )
    .bind(TriggerId::new().to_string())
    .bind(task_id.to_string())
    .execute(&pool)
    .await
    .unwrap();

    let res = task_repo.find_by_id(&task_id).await;
    assert!(res.is_err());
}

async fn setup_test_db() -> easyjob_persistence::DbPool {
    init_pool("sqlite::memory:?cache=shared").await.unwrap()
}

fn make_dummy_task(name: &str) -> Task {
    let task_id = TaskId::new();
    Task {
        id: task_id,
        name: name.to_string(),
        description: None,
        enabled: true,
        triggers: vec![],
        actions: vec![],
        execution_policy: ExecutionPolicy::default(),
        working_directory: None,
        environment: HashMap::new(),
        version: 1,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    }
}

#[tokio::test]
async fn test_settings_repo_crud() {
    let pool = setup_test_db().await;
    let repo = SqliteSettingsRepository::new(pool);

    // 1. 初始获取返回默认值 (KeepDays(7))
    let settings = repo.get_system_settings().await.unwrap();
    assert_eq!(
        settings.default_log_retention,
        SystemLogRetention::KeepDays(7)
    );

    // 2. 更新设置为 30 天
    let updated = SystemSettings {
        default_log_retention: SystemLogRetention::KeepDays(30),
    };
    repo.save_system_settings(&updated).await.unwrap();

    let fetched = repo.get_system_settings().await.unwrap();
    assert_eq!(
        fetched.default_log_retention,
        SystemLogRetention::KeepDays(30)
    );

    // 3. 更新为永久保留
    let perm = SystemSettings {
        default_log_retention: SystemLogRetention::Permanent,
    };
    repo.save_system_settings(&perm).await.unwrap();
    let fetched_perm = repo.get_system_settings().await.unwrap();
    assert_eq!(
        fetched_perm.default_log_retention,
        SystemLogRetention::Permanent
    );
}

#[tokio::test]
async fn test_purge_expired_runs_and_cascade_outputs() {
    let pool = setup_test_db().await;
    let exec_repo = SqliteExecutionRepository::new(pool.clone());
    let task_repo = SqliteTaskRepository::new(pool.clone());

    // 准备一个任务
    let task = make_dummy_task("purge-test-task");
    task_repo.save(&task).await.unwrap();

    let now = chrono::Utc::now();
    let ten_days_ago = now - chrono::Duration::days(10);
    let two_days_ago = now - chrono::Duration::days(2);

    // 1. 创建 10 天前的已完成运行及输出
    let mut old_run = Execution::new(task.id, None, Some(ten_days_ago));
    old_run.status = ExecutionStatus::Succeeded;
    old_run.started_at = ten_days_ago;
    old_run.finished_at = Some(ten_days_ago + chrono::Duration::seconds(5));
    exec_repo.create_run(&old_run).await.unwrap();
    exec_repo
        .append_output(&old_run.id, "stdout", "old log content")
        .await
        .unwrap();

    // 2. 创建 2 天前的已完成运行及输出
    let mut recent_run = Execution::new(task.id, None, Some(two_days_ago));
    recent_run.status = ExecutionStatus::Succeeded;
    recent_run.started_at = two_days_ago;
    recent_run.finished_at = Some(two_days_ago + chrono::Duration::seconds(5));
    exec_repo.create_run(&recent_run).await.unwrap();
    exec_repo
        .append_output(&recent_run.id, "stdout", "recent log content")
        .await
        .unwrap();

    // 3. 创建 10 天前但仍在 Running 状态的运行
    let mut running_old_run = Execution::new(task.id, None, Some(ten_days_ago));
    running_old_run.status = ExecutionStatus::Running;
    running_old_run.started_at = ten_days_ago;
    exec_repo.create_run(&running_old_run).await.unwrap();

    // 4. 以 7 天前为 cutoff 执行清理
    let cutoff = now - chrono::Duration::days(7);
    let deleted_count = exec_repo
        .purge_expired_runs(&task.id, cutoff)
        .await
        .unwrap();
    assert_eq!(deleted_count, 1, "只应清理 1 条 10 天前已结束的运行");

    // 5. 验证已删除旧运行与输出
    assert!(exec_repo
        .find_run_by_id(&old_run.id)
        .await
        .unwrap()
        .is_none());
    let old_outputs = exec_repo.get_outputs(&old_run.id).await.unwrap();
    assert!(old_outputs.is_empty(), "外键级联删除输出记录");

    // 6. 验证近期的运行与仍在运行中的记录完好
    assert!(exec_repo
        .find_run_by_id(&recent_run.id)
        .await
        .unwrap()
        .is_some());
    assert!(exec_repo
        .find_run_by_id(&running_old_run.id)
        .await
        .unwrap()
        .is_some());
}

async fn create_run_at(
    repo: &SqliteExecutionRepository,
    task_id: TaskId,
    started_at: chrono::DateTime<chrono::Utc>,
    status: ExecutionStatus,
) -> Execution {
    let mut exec = Execution::new(task_id, None, Some(started_at));
    exec.status = status;
    exec.started_at = started_at;
    exec.finished_at = Some(started_at + chrono::Duration::seconds(1));
    exec.duration_ms = Some(1000);
    repo.create_run(&exec).await.unwrap();
    exec
}

#[tokio::test]
async fn test_find_latest_run_per_task_returns_newest_per_task() {
    let pool = setup_test_db().await;
    let exec_repo = SqliteExecutionRepository::new(pool.clone());
    let task_repo = SqliteTaskRepository::new(pool.clone());

    let task_a = make_dummy_task("latest-a");
    let task_b = make_dummy_task("latest-b");
    let task_c = make_dummy_task("latest-c");
    task_repo.save(&task_a).await.unwrap();
    task_repo.save(&task_b).await.unwrap();
    task_repo.save(&task_c).await.unwrap();

    let base = chrono::Utc::now() - chrono::Duration::hours(3);
    create_run_at(&exec_repo, task_a.id, base, ExecutionStatus::Succeeded).await;
    let a_new = create_run_at(
        &exec_repo,
        task_a.id,
        base + chrono::Duration::hours(1),
        ExecutionStatus::Failed,
    )
    .await;
    let b_only = create_run_at(&exec_repo, task_b.id, base, ExecutionStatus::Succeeded).await;

    let result = exec_repo
        .find_latest_run_per_task(&[task_a.id, task_b.id, task_c.id])
        .await
        .unwrap();

    assert_eq!(result.len(), 2, "无运行记录的 task_c 不应出现在结果中");
    assert_eq!(result[&task_a.id].id, a_new.id);
    assert_eq!(result[&task_a.id].status, ExecutionStatus::Failed);
    assert_eq!(result[&task_b.id].id, b_only.id);
    assert!(!result.contains_key(&task_c.id));

    // 空入参不应构造非法 SQL
    assert!(exec_repo
        .find_latest_run_per_task(&[])
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn test_find_latest_run_per_task_tie_breaks_by_id_desc() {
    let pool = setup_test_db().await;
    let exec_repo = SqliteExecutionRepository::new(pool.clone());
    let task_repo = SqliteTaskRepository::new(pool.clone());
    let task = make_dummy_task("latest-tie");
    task_repo.save(&task).await.unwrap();

    let started_at = chrono::Utc::now() - chrono::Duration::hours(1);
    let mut exec_a = Execution::new(task.id, None, Some(started_at));
    exec_a.status = ExecutionStatus::Succeeded;
    exec_a.started_at = started_at;
    let mut exec_b = Execution::new(task.id, None, Some(started_at));
    exec_b.status = ExecutionStatus::Failed;
    exec_b.started_at = started_at;

    // 先插入较大 id 的那条，再插入较小 id 的那条：
    // 这样「按插入顺序取最后一条」会取到较小 id，与期望不同，
    // 从而真正锁住 id DESC 的次级排序。
    let (bigger, smaller) = if exec_a.id.to_string() > exec_b.id.to_string() {
        (exec_a, exec_b)
    } else {
        (exec_b, exec_a)
    };
    let expected_id = bigger.id;
    let expected_status = bigger.status;
    exec_repo.create_run(&bigger).await.unwrap();
    exec_repo.create_run(&smaller).await.unwrap();

    let result = exec_repo
        .find_latest_run_per_task(&[task.id])
        .await
        .unwrap();
    assert_eq!(result[&task.id].id, expected_id);
    assert_eq!(result[&task.id].status, expected_status);
}
