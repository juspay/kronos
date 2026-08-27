use crate::{db::DbContext, models::Execution};
use sqlx::prelude::FromRow;

#[derive(FromRow)]
pub struct ClaimedExecution {
    pub execution_id: String,
    pub job_id: String,
    pub endpoint: String,
    pub endpoint_type: String,
    pub input: Option<serde_json::Value>,
    pub attempt_count: i64,
    pub max_attempts: i64,
}

pub async fn claim(
    db: &mut DbContext<'_>,
    worker_id: &str,
) -> Result<Option<ClaimedExecution>, sqlx::Error> {
    let t = db.tbl("executions");
    let row: Option<ClaimedExecution> = sqlx::query_as(&format!(
        "UPDATE {t}
         SET status = 'RUNNING',
             worker_id = $1,
             started_at = now(),
             attempt_count = attempt_count + 1
         WHERE execution_id = (
             SELECT execution_id
             FROM {t}
             WHERE status IN ('QUEUED', 'RETRYING', 'PENDING')
               AND run_at <= now()
             ORDER BY run_at ASC
             LIMIT 1
             FOR UPDATE SKIP LOCKED
         )
         RETURNING execution_id, job_id, endpoint, endpoint_type, input, attempt_count, max_attempts"
    ))
    .bind(worker_id)
    .fetch_optional(&mut *db.conn)
    .await?;

    Ok(row)
}

pub async fn complete_success(
    db: &mut DbContext<'_>,
    execution_id: &str,
    output: &serde_json::Value,
) -> Result<(), sqlx::Error> {
    let t = db.tbl("executions");
    sqlx::query(&format!(
        "UPDATE {t}
         SET status = 'SUCCESS', output = $2, completed_at = now(),
             duration_ms = (EXTRACT(EPOCH FROM (now() - started_at)) * 1000)::BIGINT
         WHERE execution_id = $1 AND status = 'RUNNING'"
    ))
    .bind(execution_id)
    .bind(output)
    .execute(&mut *db.conn)
    .await?;
    Ok(())
}

pub async fn complete_retry(
    db: &mut DbContext<'_>,
    execution_id: &str,
    backoff_ms: i64,
) -> Result<(), sqlx::Error> {
    let t = db.tbl("executions");
    sqlx::query(&format!(
        "UPDATE {t}
         SET status = CASE WHEN attempt_count >= max_attempts THEN 'FAILED' ELSE 'RETRYING' END,
             run_at = CASE WHEN attempt_count >= max_attempts THEN run_at
                      ELSE now() + ($2 * interval '1 millisecond') END,
             worker_id = NULL,
             completed_at = CASE WHEN attempt_count >= max_attempts THEN now() ELSE NULL END,
             duration_ms = CASE WHEN attempt_count >= max_attempts
                           THEN (EXTRACT(EPOCH FROM (now() - started_at)) * 1000)::BIGINT
                           ELSE NULL END
         WHERE execution_id = $1 AND status = 'RUNNING'"
    ))
    .bind(execution_id)
    .bind(backoff_ms)
    .execute(&mut *db.conn)
    .await?;
    Ok(())
}

pub async fn complete_failed(
    db: &mut DbContext<'_>,
    execution_id: &str,
) -> Result<(), sqlx::Error> {
    let t = db.tbl("executions");
    sqlx::query(&format!(
        "UPDATE {t}
         SET status = 'FAILED', completed_at = now(),
             duration_ms = (EXTRACT(EPOCH FROM (now() - started_at)) * 1000)::BIGINT,
             worker_id = NULL
         WHERE execution_id = $1 AND status = 'RUNNING'"
    ))
    .bind(execution_id)
    .execute(&mut *db.conn)
    .await?;
    Ok(())
}

pub async fn get(
    db: &mut DbContext<'_>,
    execution_id: &str,
) -> Result<Option<Execution>, sqlx::Error> {
    let t = db.tbl("executions");
    sqlx::query_as::<_, Execution>(&format!("SELECT * FROM {t} WHERE execution_id = $1"))
        .bind(execution_id)
        .fetch_optional(&mut *db.conn)
        .await
}

pub async fn get_for_job(
    db: &mut DbContext<'_>,
    job_id: &str,
) -> Result<Option<Execution>, sqlx::Error> {
    let t = db.tbl("executions");
    sqlx::query_as::<_, Execution>(&format!(
        "SELECT * FROM {t} WHERE job_id = $1 ORDER BY created_at DESC LIMIT 1"
    ))
    .bind(job_id)
    .fetch_optional(&mut *db.conn)
    .await
}

pub async fn list_for_job(
    db: &mut DbContext<'_>,
    job_id: &str,
    cursor: Option<&str>,
    limit: i64,
) -> Result<Vec<Execution>, sqlx::Error> {
    let t = db.tbl("executions");
    match cursor {
        Some(c) => {
            sqlx::query_as::<_, Execution>(&format!(
                "SELECT * FROM {t}
                 WHERE job_id = $1 AND created_at < (SELECT created_at FROM {t} WHERE execution_id = $2)
                 ORDER BY created_at DESC LIMIT $3"
            ))
            .bind(job_id)
            .bind(c)
            .bind(limit)
            .fetch_all(&mut *db.conn)
            .await
        }
        None => {
            sqlx::query_as::<_, Execution>(&format!(
                "SELECT * FROM {t} WHERE job_id = $1 ORDER BY created_at DESC LIMIT $2"
            ))
            .bind(job_id)
            .bind(limit)
            .fetch_all(&mut *db.conn)
            .await
        }
    }
}

pub async fn cancel(
    db: &mut DbContext<'_>,
    execution_id: &str,
) -> Result<Option<Execution>, sqlx::Error> {
    let t = db.tbl("executions");
    sqlx::query_as::<_, Execution>(&format!(
        "UPDATE {t} SET status = 'CANCELLED', completed_at = now()
         WHERE execution_id = $1 AND status IN ('PENDING', 'QUEUED')
         RETURNING *"
    ))
    .bind(execution_id)
    .fetch_optional(&mut *db.conn)
    .await
}

pub async fn cancel_pending_for_job(
    db: &mut DbContext<'_>,
    job_id: &str,
) -> Result<u64, sqlx::Error> {
    let t = db.tbl("executions");
    let result = sqlx::query(&format!(
        "UPDATE {t} SET status = 'CANCELLED', completed_at = now()
         WHERE job_id = $1 AND status IN ('PENDING', 'QUEUED')"
    ))
    .bind(job_id)
    .execute(&mut *db.conn)
    .await?;
    Ok(result.rows_affected())
}
