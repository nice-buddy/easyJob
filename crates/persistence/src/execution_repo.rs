use async_trait::async_trait;
use chrono::Utc;
use easyjob_common::{Error, ExecutionId, Result, TaskId, TriggerId};
use easyjob_domain::execution::{Execution, ExecutionStatus};
use sqlx::{Row, SqlitePool};

#[async_trait]
pub trait ExecutionRepository: Send + Sync {
    async fn create_run(&self, run: &Execution) -> Result<()>;
    async fn update_run(&self, run: &Execution) -> Result<()>;
    async fn find_run_by_id(&self, id: &ExecutionId) -> Result<Option<Execution>>;
    async fn find_recent_runs(&self, limit: u32) -> Result<Vec<Execution>>;
    async fn append_output(&self, run_id: &ExecutionId, stream: &str, content: &str) -> Result<()>;
}

pub struct SqliteExecutionRepository {
    pool: SqlitePool,
}

impl SqliteExecutionRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl ExecutionRepository for SqliteExecutionRepository {
    async fn create_run(&self, run: &Execution) -> Result<()> {
        sqlx::query(
            "INSERT INTO task_runs (id, task_id, trigger_id, status, scheduled_at, started_at, finished_at, duration_ms, exit_code, error_message)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"
        )
        .bind(run.id.to_string())
        .bind(run.task_id.to_string())
        .bind(run.trigger_id.map(|t| t.to_string()))
        .bind(serde_json::to_string(&run.status)?)
        .bind(run.scheduled_at.map(|s| s.to_rfc3339()))
        .bind(run.started_at.to_rfc3339())
        .bind(run.finished_at.map(|f| f.to_rfc3339()))
        .bind(run.duration_ms.map(|d| d as i64))
        .bind(run.exit_code)
        .bind(&run.error_message)
        .execute(&self.pool)
        .await
        .map_err(|e| Error::Database(e.to_string()))?;
        Ok(())
    }

    async fn update_run(&self, run: &Execution) -> Result<()> {
        sqlx::query(
            "UPDATE task_runs SET
                status = ?,
                finished_at = ?,
                duration_ms = ?,
                exit_code = ?,
                error_message = ?
             WHERE id = ?",
        )
        .bind(serde_json::to_string(&run.status)?)
        .bind(run.finished_at.map(|f| f.to_rfc3339()))
        .bind(run.duration_ms.map(|d| d as i64))
        .bind(run.exit_code)
        .bind(&run.error_message)
        .bind(run.id.to_string())
        .execute(&self.pool)
        .await
        .map_err(|e| Error::Database(e.to_string()))?;
        Ok(())
    }

    async fn find_run_by_id(&self, id: &ExecutionId) -> Result<Option<Execution>> {
        let row = sqlx::query("SELECT * FROM task_runs WHERE id = ?")
            .bind(id.to_string())
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| Error::Database(e.to_string()))?;

        match row {
            Some(r) => map_row_to_execution(&r).map(Some),
            None => Ok(None),
        }
    }

    async fn find_recent_runs(&self, limit: u32) -> Result<Vec<Execution>> {
        let rows = sqlx::query("SELECT * FROM task_runs ORDER BY started_at DESC LIMIT ?")
            .bind(limit as i64)
            .fetch_all(&self.pool)
            .await
            .map_err(|e| Error::Database(e.to_string()))?;

        let mut runs = Vec::with_capacity(rows.len());
        for row in rows {
            runs.push(map_row_to_execution(&row)?);
        }
        Ok(runs)
    }

    async fn append_output(&self, run_id: &ExecutionId, stream: &str, content: &str) -> Result<()> {
        sqlx::query(
            "INSERT INTO run_outputs (run_id, stream, content, created_at) VALUES (?, ?, ?, ?)",
        )
        .bind(run_id.to_string())
        .bind(stream)
        .bind(content)
        .bind(Utc::now().to_rfc3339())
        .execute(&self.pool)
        .await
        .map_err(|e| Error::Database(e.to_string()))?;
        Ok(())
    }
}

fn map_row_to_execution(r: &sqlx::sqlite::SqliteRow) -> Result<Execution> {
    let id_str: String = r.get("id");
    let status_json: String = r.get("status");
    let status: ExecutionStatus = serde_json::from_str(&status_json)?;
    let task_id_str: String = r.get("task_id");
    let trigger_id_str: Option<String> = r.get("trigger_id");
    let sched_str: Option<String> = r.get("scheduled_at");
    let start_str: String = r.get("started_at");
    let finish_str: Option<String> = r.get("finished_at");
    let dur: Option<i64> = r.get("duration_ms");

    Ok(Execution {
        id: ExecutionId::parse(&id_str).map_err(|e| Error::Database(e.to_string()))?,
        task_id: TaskId::parse(&task_id_str).map_err(|e| Error::Database(e.to_string()))?,
        trigger_id: trigger_id_str.and_then(|s| TriggerId::parse(&s).ok()),
        status,
        scheduled_at: sched_str.and_then(|s| s.parse().ok()),
        started_at: start_str.parse().unwrap_or_else(|_| Utc::now()),
        finished_at: finish_str.and_then(|s| s.parse().ok()),
        duration_ms: dur.map(|d| d as u64),
        exit_code: r.get("exit_code"),
        error_message: r.get("error_message"),
    })
}
