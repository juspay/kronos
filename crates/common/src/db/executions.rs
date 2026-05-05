use crate::{db::tbl, models::Execution};
use chrono::{DateTime, Utc};
use sqlx::{prelude::FromRow, PgConnection};

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
    conn: &mut PgConnection,
    prefix: &str,
    worker_id: &str,
) -> Result<Option<ClaimedExecution>, sqlx::Error> {
    let t = tbl(prefix, "executions");
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
    .fetch_optional(&mut *conn)
    .await?;

    Ok(row)
}

pub async fn complete_success(
    conn: &mut PgConnection,
    prefix: &str,
    execution_id: &str,
    output: &serde_json::Value,
) -> Result<(), sqlx::Error> {
    let t = tbl(prefix, "executions");
    sqlx::query(&format!(
        "UPDATE {t}
         SET status = 'SUCCESS', output = $2, completed_at = now(),
             duration_ms = (EXTRACT(EPOCH FROM (now() - started_at)) * 1000)::BIGINT
         WHERE execution_id = $1 AND status = 'RUNNING'"
    ))
    .bind(execution_id)
    .bind(output)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

pub async fn complete_retry(
    conn: &mut PgConnection,
    prefix: &str,
    execution_id: &str,
    backoff_ms: i64,
) -> Result<(), sqlx::Error> {
    let t = tbl(prefix, "executions");
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
    .execute(&mut *conn)
    .await?;
    Ok(())
}

pub async fn complete_failed(
    conn: &mut PgConnection,
    prefix: &str,
    execution_id: &str,
) -> Result<(), sqlx::Error> {
    let t = tbl(prefix, "executions");
    sqlx::query(&format!(
        "UPDATE {t}
         SET status = 'FAILED', completed_at = now(),
             duration_ms = (EXTRACT(EPOCH FROM (now() - started_at)) * 1000)::BIGINT,
             worker_id = NULL
         WHERE execution_id = $1 AND status = 'RUNNING'"
    ))
    .bind(execution_id)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

pub async fn get(
    conn: &mut PgConnection,
    prefix: &str,
    execution_id: &str,
) -> Result<Option<Execution>, sqlx::Error> {
    let t = tbl(prefix, "executions");
    sqlx::query_as::<_, Execution>(&format!("SELECT * FROM {t} WHERE execution_id = $1"))
        .bind(execution_id)
        .fetch_optional(&mut *conn)
        .await
}

pub async fn get_for_job(
    conn: &mut PgConnection,
    prefix: &str,
    job_id: &str,
) -> Result<Option<Execution>, sqlx::Error> {
    let t = tbl(prefix, "executions");
    sqlx::query_as::<_, Execution>(&format!(
        "SELECT * FROM {t} WHERE job_id = $1 ORDER BY created_at DESC LIMIT 1"
    ))
    .bind(job_id)
    .fetch_optional(&mut *conn)
    .await
}

pub async fn list_for_job(
    conn: &mut PgConnection,
    prefix: &str,
    job_id: &str,
    cursor: Option<&str>,
    limit: i64,
) -> Result<Vec<Execution>, sqlx::Error> {
    let t = tbl(prefix, "executions");
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
            .fetch_all(&mut *conn)
            .await
        }
        None => {
            sqlx::query_as::<_, Execution>(&format!(
                "SELECT * FROM {t} WHERE job_id = $1 ORDER BY created_at DESC LIMIT $2"
            ))
            .bind(job_id)
            .bind(limit)
            .fetch_all(&mut *conn)
            .await
        }
    }
}

pub async fn cancel(
    conn: &mut PgConnection,
    prefix: &str,
    execution_id: &str,
) -> Result<Option<Execution>, sqlx::Error> {
    let t = tbl(prefix, "executions");
    sqlx::query_as::<_, Execution>(&format!(
        "UPDATE {t} SET status = 'CANCELLED', completed_at = now()
         WHERE execution_id = $1 AND status IN ('PENDING', 'QUEUED')
         RETURNING *"
    ))
    .bind(execution_id)
    .fetch_optional(&mut *conn)
    .await
}

pub async fn create_cron_execution(
    conn: &mut PgConnection,
    prefix: &str,
    job_id: &str,
    endpoint: &str,
    endpoint_type: &str,
    idempotency_key: &str,
    input: Option<&serde_json::Value>,
    run_at: DateTime<Utc>,
    max_attempts: i64,
) -> Result<bool, sqlx::Error> {
    let t = tbl(prefix, "executions");
    let result = sqlx::query(&format!(
        "INSERT INTO {t} (job_id, endpoint, endpoint_type, idempotency_key, status, input, run_at, max_attempts)
         VALUES ($1, $2, $3, $4, 'QUEUED', $5, $6, $7)
         ON CONFLICT (job_id, idempotency_key) WHERE idempotency_key IS NOT NULL DO NOTHING"
    ))
    .bind(job_id)
    .bind(endpoint)
    .bind(endpoint_type)
    .bind(idempotency_key)
    .bind(input)
    .bind(run_at)
    .bind(max_attempts)
    .execute(&mut *conn)
    .await?;
    Ok(result.rows_affected() > 0)
}

pub async fn cancel_pending_for_job(
    conn: &mut PgConnection,
    prefix: &str,
    job_id: &str,
) -> Result<u64, sqlx::Error> {
    let t = tbl(prefix, "executions");
    let result = sqlx::query(&format!(
        "UPDATE {t} SET status = 'CANCELLED', completed_at = now()
         WHERE job_id = $1 AND status IN ('PENDING', 'QUEUED')"
    ))
    .bind(job_id)
    .execute(&mut *conn)
    .await?;
    Ok(result.rows_affected())
}
