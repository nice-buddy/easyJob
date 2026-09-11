use easyjob_persistence::db::{init_pool, run_migrations};
use sqlx::Row;

#[tokio::test]
async fn test_in_memory_db_migrations() {
    let pool = init_pool("sqlite::memory:").await.unwrap();
    let row: (i64,) = sqlx::query_as("SELECT count(*) FROM tasks")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(row.0, 0);
}

#[tokio::test]
async fn test_run_migrations_idempotency() {
    let pool = init_pool("sqlite::memory:").await.unwrap();
    // Running migrations a second time should be idempotent and succeed
    run_migrations(&pool).await.unwrap();
    let row: (i64,) = sqlx::query_as("SELECT count(*) FROM tasks")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(row.0, 0);
}

#[tokio::test]
async fn test_file_db_wal_mode_and_foreign_keys_and_cascade() {
    let temp_dir = std::env::temp_dir();
    let db_path = temp_dir.join(format!("easyjob_test_{}.db", uuid::Uuid::new_v4()));
    let db_url = format!("sqlite://{}", db_path.to_string_lossy());

    let pool = init_pool(&db_url).await.unwrap();

    // Verify WAL mode
    let journal_mode_row = sqlx::query("PRAGMA journal_mode")
        .fetch_one(&pool)
        .await
        .unwrap();
    let journal_mode: String = journal_mode_row.get(0);
    assert_eq!(journal_mode.to_lowercase(), "wal");

    // Verify foreign keys enabled
    let fk_row = sqlx::query("PRAGMA foreign_keys")
        .fetch_one(&pool)
        .await
        .unwrap();
    let fk_enabled: i64 = fk_row.get(0);
    assert_eq!(fk_enabled, 1);

    // Verify all 7 tables exist
    let table_count: (i64,) = sqlx::query_as(
        "SELECT count(*) FROM sqlite_master WHERE type='table' AND name IN (
            'tasks', 'triggers', 'actions', 'task_runs', 'run_outputs', 'agent_state', 'settings'
        )",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(table_count.0, 7);

    // Test foreign key cascading deletes
    sqlx::query(
        "INSERT INTO tasks (id, name, description, enabled, execution_policy_json, working_directory, environment_json, version, created_at, updated_at)
         VALUES ('t-1', 'Test Task', NULL, 1, '{}', NULL, '{}', 1, '2026-09-11T00:00:00Z', '2026-09-11T00:00:00Z')"
    )
    .execute(&pool)
    .await
    .unwrap();

    sqlx::query(
        "INSERT INTO triggers (id, task_id, kind, config_json, enabled, created_at, updated_at)
         VALUES ('tr-1', 't-1', 'Manual', '{}', 1, '2026-09-11T00:00:00Z', '2026-09-11T00:00:00Z')",
    )
    .execute(&pool)
    .await
    .unwrap();

    sqlx::query(
        "INSERT INTO actions (id, task_id, sequence, kind, config_json, enabled)
         VALUES ('act-1', 't-1', 0, 'Command', '{\"program\":\"echo\"}', 1)",
    )
    .execute(&pool)
    .await
    .unwrap();

    sqlx::query(
        "INSERT INTO task_runs (id, task_id, trigger_id, status, scheduled_at, started_at, finished_at, duration_ms, exit_code, error_message)
         VALUES ('run-1', 't-1', 'tr-1', 'Success', NULL, '2026-09-11T00:00:00Z', '2026-09-11T00:00:01Z', 1000, 0, NULL)"
    )
    .execute(&pool)
    .await
    .unwrap();

    sqlx::query(
        "INSERT INTO run_outputs (run_id, stream, content, created_at)
         VALUES ('run-1', 'stdout', 'hello world', '2026-09-11T00:00:01Z')",
    )
    .execute(&pool)
    .await
    .unwrap();

    // Verify rows exist before delete
    let trigger_count: (i64,) =
        sqlx::query_as("SELECT count(*) FROM triggers WHERE task_id = 't-1'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(trigger_count.0, 1);

    let output_count: (i64,) =
        sqlx::query_as("SELECT count(*) FROM run_outputs WHERE run_id = 'run-1'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(output_count.0, 1);

    // Delete task and verify cascades
    sqlx::query("DELETE FROM tasks WHERE id = 't-1'")
        .execute(&pool)
        .await
        .unwrap();

    let trigger_count_after: (i64,) =
        sqlx::query_as("SELECT count(*) FROM triggers WHERE task_id = 't-1'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(trigger_count_after.0, 0);

    let action_count_after: (i64,) =
        sqlx::query_as("SELECT count(*) FROM actions WHERE task_id = 't-1'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(action_count_after.0, 0);

    let run_count_after: (i64,) =
        sqlx::query_as("SELECT count(*) FROM task_runs WHERE task_id = 't-1'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(run_count_after.0, 0);

    let output_count_after: (i64,) =
        sqlx::query_as("SELECT count(*) FROM run_outputs WHERE run_id = 'run-1'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(output_count_after.0, 0);

    // Clean up temporary database files
    drop(pool);
    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(format!("{}-wal", db_path.to_string_lossy()));
    let _ = std::fs::remove_file(format!("{}-shm", db_path.to_string_lossy()));
}

#[tokio::test]
async fn test_agent_state_and_settings_tables() {
    let pool = init_pool("sqlite::memory:").await.unwrap();

    sqlx::query("INSERT INTO agent_state (key, value_json, updated_at) VALUES ('version', '\"1.0.0\"', '2026-09-11T00:00:00Z')")
        .execute(&pool)
        .await
        .unwrap();

    sqlx::query("INSERT INTO settings (key, value_json, updated_at) VALUES ('theme', '\"dark\"', '2026-09-11T00:00:00Z')")
        .execute(&pool)
        .await
        .unwrap();

    let state_row: (String, String) =
        sqlx::query_as("SELECT key, value_json FROM agent_state WHERE key = 'version'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(state_row.0, "version");
    assert_eq!(state_row.1, "\"1.0.0\"");

    let setting_row: (String, String) =
        sqlx::query_as("SELECT key, value_json FROM settings WHERE key = 'theme'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(setting_row.0, "theme");
    assert_eq!(setting_row.1, "\"dark\"");
}
