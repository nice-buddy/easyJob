use async_trait::async_trait;
use chrono::Utc;
use easyjob_common::{Error, ExecutionId, Result, TaskId, TriggerId};
use easyjob_domain::execution::{Execution, ExecutionStatus};
use sqlx::{Row, SqlitePool};
use std::collections::HashMap;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ExecutionOutputRecord {
    pub stream: String,
    pub content: String,
    pub created_at: String,
}

#[async_trait]
pub trait ExecutionRepository: Send + Sync {
    async fn create_run(&self, run: &Execution) -> Result<()>;
    async fn update_run(&self, run: &Execution) -> Result<()>;
    async fn find_run_by_id(&self, id: &ExecutionId) -> Result<Option<Execution>>;
    async fn find_recent_runs(&self, limit: u32) -> Result<Vec<Execution>>;
    async fn append_output(&self, run_id: &ExecutionId, stream: &str, content: &str) -> Result<()>;
    async fn get_outputs(&self, run_id: &ExecutionId) -> Result<Vec<ExecutionOutputRecord>>;
    async fn purge_expired_runs(
        &self,
        task_id: &TaskId,
        before: chrono::DateTime<Utc>,
    ) -> Result<u64>;
    /// 批量查询每个任务最近一次执行（无运行记录的任务不会出现在结果中）。
    async fn find_latest_run_per_task(
        &self,
        task_ids: &[TaskId],
    ) -> Result<HashMap<TaskId, Execution>>;
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

    async fn get_outputs(&self, run_id: &ExecutionId) -> Result<Vec<ExecutionOutputRecord>> {
        let rows = sqlx::query(
            "SELECT stream, content, created_at FROM run_outputs WHERE run_id = ? ORDER BY id ASC",
        )
        .bind(run_id.to_string())
        .fetch_all(&self.pool)
        .await
        .map_err(|e| Error::Database(e.to_string()))?;

        let mut list = Vec::with_capacity(rows.len());
        for row in rows {
            list.push(ExecutionOutputRecord {
                stream: row.get("stream"),
                content: row.get("content"),
                created_at: row.get("created_at"),
            });
        }
        Ok(list)
    }

    async fn purge_expired_runs(
        &self,
        task_id: &TaskId,
        before: chrono::DateTime<Utc>,
    ) -> Result<u64> {
        let result = sqlx::query(
            "DELETE FROM task_runs
             WHERE task_id = ?
               AND started_at < ?
               AND status != '\"Running\"'",
        )
        .bind(task_id.to_string())
        .bind(before.to_rfc3339())
        .execute(&self.pool)
        .await
        .map_err(|e| Error::Database(e.to_string()))?;

        Ok(result.rows_affected())
    }

    async fn find_latest_run_per_task(
        &self,
        task_ids: &[TaskId],
    ) -> Result<HashMap<TaskId, Execution>> {
        if task_ids.is_empty() {
            return Ok(HashMap::new());
        }

        let placeholders = vec!["?"; task_ids.len()].join(", ");
        let sql = format!(
            "SELECT * FROM (
                 SELECT *,
                        ROW_NUMBER() OVER (
                            PARTITION BY task_id ORDER BY started_at DESC, id DESC
                        ) AS row_num
                 FROM task_runs
                 WHERE task_id IN ({placeholders})
             ) WHERE row_num = 1"
        );

        let mut query = sqlx::query(&sql);
        for id in task_ids {
            query = query.bind(id.to_string());
        }
        let rows = query
            .fetch_all(&self.pool)
            .await
            .map_err(|e| Error::Database(e.to_string()))?;

        let mut latest = HashMap::with_capacity(rows.len());
        for row in &rows {
            let exec = map_row_to_execution(row)?;
            latest.insert(exec.task_id, exec);
        }
        Ok(latest)
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
