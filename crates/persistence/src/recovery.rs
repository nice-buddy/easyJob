use easyjob_common::{Error, Result};
use easyjob_domain::execution::ExecutionStatus;
use sqlx::SqlitePool;

pub async fn recover_dangling_executions(pool: &SqlitePool) -> Result<u64> {
    let running_status = serde_json::to_string(&ExecutionStatus::Running)?;
    let interrupted_status = serde_json::to_string(&ExecutionStatus::Interrupted)?;

    let res = sqlx::query(
        "UPDATE task_runs SET status = ?, error_message = 'Agent restarted unexpectedly'
         WHERE status = ?",
    )
    .bind(interrupted_status)
    .bind(running_status)
    .execute(pool)
    .await
    .map_err(|e| Error::Database(e.to_string()))?;

    Ok(res.rows_affected())
}
