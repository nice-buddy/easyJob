use chrono::Utc;
use easyjob_common::{ActionId, TaskId, TriggerId};
use easyjob_domain::action::{Action, ActionKind};
use easyjob_domain::execution::{Execution, ExecutionStatus};
use easyjob_domain::policy::ExecutionPolicy;
use easyjob_domain::task::Task;
use easyjob_domain::trigger::{Trigger, TriggerKind};
use easyjob_persistence::db::init_pool;
use easyjob_persistence::execution_repo::{ExecutionRepository, SqliteExecutionRepository};
use easyjob_persistence::recovery::recover_dangling_executions;
use easyjob_persistence::task_repo::{SqliteTaskRepository, TaskRepository};
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
