use async_trait::async_trait;
use chrono::Utc;
use easyjob_common::{Error, Result, TaskId};
use easyjob_domain::action::Action;
use easyjob_domain::task::Task;
use easyjob_domain::trigger::Trigger;
use sqlx::{Row, SqlitePool};
use std::path::PathBuf;

#[async_trait]
pub trait TaskRepository: Send + Sync {
    async fn find_all_enabled(&self) -> Result<Vec<Task>>;
    async fn find_by_id(&self, id: &TaskId) -> Result<Option<Task>>;
    async fn save(&self, task: &Task) -> Result<()>;
    async fn delete(&self, id: &TaskId) -> Result<()>;
}

pub struct SqliteTaskRepository {
    pool: SqlitePool,
}

impl SqliteTaskRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl TaskRepository for SqliteTaskRepository {
    async fn find_all_enabled(&self) -> Result<Vec<Task>> {
        let rows = sqlx::query("SELECT id FROM tasks WHERE enabled = 1")
            .fetch_all(&self.pool)
            .await
            .map_err(|e| Error::Database(e.to_string()))?;

        let mut tasks = Vec::new();
        for row in rows {
            let id_str: String = row.get("id");
            if let Ok(id) = TaskId::parse(&id_str) {
                if let Some(task) = self.find_by_id(&id).await? {
                    tasks.push(task);
                }
            }
        }
        Ok(tasks)
    }

    async fn find_by_id(&self, id: &TaskId) -> Result<Option<Task>> {
        let task_row = sqlx::query(
            "SELECT id, name, description, enabled, execution_policy_json, working_directory, environment_json, version, created_at, updated_at
             FROM tasks WHERE id = ?"
        )
        .bind(id.to_string())
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| Error::Database(e.to_string()))?;

        let row = match task_row {
            Some(r) => r,
            None => return Ok(None),
        };

        let trigger_rows = sqlx::query("SELECT config_json FROM triggers WHERE task_id = ?")
            .bind(id.to_string())
            .fetch_all(&self.pool)
            .await
            .map_err(|e| Error::Database(e.to_string()))?;

        let mut triggers = Vec::new();
        for tr in trigger_rows {
            let json: String = tr.get("config_json");
            let t = serde_json::from_str::<Trigger>(&json)?;
            triggers.push(t);
        }

        let action_rows =
            sqlx::query("SELECT config_json FROM actions WHERE task_id = ? ORDER BY sequence ASC")
                .bind(id.to_string())
                .fetch_all(&self.pool)
                .await
                .map_err(|e| Error::Database(e.to_string()))?;

        let mut actions = Vec::new();
        for ar in action_rows {
            let json: String = ar.get("config_json");
            let a = serde_json::from_str::<Action>(&json)?;
            actions.push(a);
        }

        let policy_json: String = row.get("execution_policy_json");
        let env_json: String = row.get("environment_json");
        let wd: Option<String> = row.get("working_directory");

        let task = Task {
            id: *id,
            name: row.get("name"),
            description: row.get("description"),
            enabled: row.get::<i64, _>("enabled") == 1,
            triggers,
            actions,
            execution_policy: serde_json::from_str(&policy_json)?,
            working_directory: wd.map(PathBuf::from),
            environment: serde_json::from_str(&env_json)?,
            version: row.get("version"),
            created_at: row
                .get::<String, _>("created_at")
                .parse()
                .unwrap_or_else(|_| Utc::now()),
            updated_at: row
                .get::<String, _>("updated_at")
                .parse()
                .unwrap_or_else(|_| Utc::now()),
        };

        Ok(Some(task))
    }

    async fn save(&self, task: &Task) -> Result<()> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| Error::Database(e.to_string()))?;

        let policy_json = serde_json::to_string(&task.execution_policy)?;
        let env_json = serde_json::to_string(&task.environment)?;
        let wd_str = task
            .working_directory
            .as_ref()
            .map(|p| p.to_string_lossy().to_string());

        sqlx::query(
            "INSERT INTO tasks (id, name, description, enabled, execution_policy_json, working_directory, environment_json, version, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(id) DO UPDATE SET
                name = excluded.name,
                description = excluded.description,
                enabled = excluded.enabled,
                execution_policy_json = excluded.execution_policy_json,
                working_directory = excluded.working_directory,
                environment_json = excluded.environment_json,
                version = tasks.version + 1,
                updated_at = excluded.updated_at"
        )
        .bind(task.id.to_string())
        .bind(&task.name)
        .bind(&task.description)
        .bind(if task.enabled { 1 } else { 0 })
        .bind(policy_json)
        .bind(wd_str)
        .bind(env_json)
        .bind(task.version)
        .bind(task.created_at.to_rfc3339())
        .bind(task.updated_at.to_rfc3339())
        .execute(&mut *tx)
        .await
        .map_err(|e| Error::Database(e.to_string()))?;

        sqlx::query("DELETE FROM triggers WHERE task_id = ?")
            .bind(task.id.to_string())
            .execute(&mut *tx)
            .await
            .map_err(|e| Error::Database(e.to_string()))?;

        for trigger in &task.triggers {
            let kind_str = format!("{:?}", trigger.kind);
            let config_json = serde_json::to_string(trigger)?;
            sqlx::query(
                "INSERT INTO triggers (id, task_id, kind, config_json, enabled, created_at, updated_at)
                 VALUES (?, ?, ?, ?, ?, ?, ?)"
            )
            .bind(trigger.id.to_string())
            .bind(task.id.to_string())
            .bind(kind_str)
            .bind(config_json)
            .bind(if trigger.enabled { 1 } else { 0 })
            .bind(trigger.created_at.to_rfc3339())
            .bind(trigger.updated_at.to_rfc3339())
            .execute(&mut *tx)
            .await
            .map_err(|e| Error::Database(e.to_string()))?;
        }

        sqlx::query("DELETE FROM actions WHERE task_id = ?")
            .bind(task.id.to_string())
            .execute(&mut *tx)
            .await
            .map_err(|e| Error::Database(e.to_string()))?;

        for action in &task.actions {
            let kind_str = format!("{:?}", action.kind);
            let config_json = serde_json::to_string(action)?;
            sqlx::query(
                "INSERT INTO actions (id, task_id, sequence, kind, config_json, enabled)
                 VALUES (?, ?, ?, ?, ?, ?)",
            )
            .bind(action.id.to_string())
            .bind(task.id.to_string())
            .bind(action.sequence)
            .bind(kind_str)
            .bind(config_json)
            .bind(if action.enabled { 1 } else { 0 })
            .execute(&mut *tx)
            .await
            .map_err(|e| Error::Database(e.to_string()))?;
        }

        tx.commit()
            .await
            .map_err(|e| Error::Database(e.to_string()))?;
        Ok(())
    }

    async fn delete(&self, id: &TaskId) -> Result<()> {
        sqlx::query("DELETE FROM tasks WHERE id = ?")
            .bind(id.to_string())
            .execute(&self.pool)
            .await
            .map_err(|e| Error::Database(e.to_string()))?;
        Ok(())
    }
}
