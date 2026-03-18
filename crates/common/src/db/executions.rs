use crate::models::Execution;
use chrono::{DateTime, Utc};
use sqlx::PgPool;

pub struct ClaimedExecution {
    pub execution_id: String,
    pub job_id: String,
    pub endpoint: String,
    pub endpoint_type: String,
    pub input: Option<serde_json::Value>,
    pub attempt_count: i64,
    pub max_attempts: i64,
}

impl ClaimedExecution {
    fn from_row(row: (String, String, String, String, Option<serde_json::Value>, i64, i64)) -> Self {
        Self {
            execution_id: row.0,
            job_id: row.1,
            endpoint: row.2,
            endpoint_type: row.3,
            input: row.4,
            attempt_count: row.5,
            max_attempts: row.6,
        }
    }
}

pub async fn claim(pool: &PgPool, worker_id: &str) -> Result<Option<ClaimedExecution>, sqlx::Error> {
    let row: Option<(String, String, String, String, Option<serde_json::Value>, i64, i64)> = sqlx::query_as(
        "UPDATE executions
         SET status = 'RUNNING',
             worker_id = $1,
             started_at = now(),
             attempt_count = attempt_count + 1
         WHERE execution_id = (
             SELECT execution_id
             FROM executions
             WHERE status IN ('QUEUED', 'RETRYING')
               AND run_at <= now()
             ORDER BY run_at ASC
             LIMIT 1
             FOR UPDATE SKIP LOCKED
         )
         RETURNING execution_id, job_id, endpoint, endpoint_type, input, attempt_count, max_attempts"
    )
    .bind(worker_id)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(ClaimedExecution::from_row))
}

pub async fn complete_success(
    pool: &PgPool,
    execution_id: &str,
    output: &serde_json::Value,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE executions
         SET status = 'SUCCESS', output = $2, completed_at = now(),
             duration_ms = (EXTRACT(EPOCH FROM (now() - started_at)) * 1000)::INT
         WHERE execution_id = $1 AND status = 'RUNNING'"
    )
    .bind(execution_id)
    .bind(output)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn complete_retry(
    pool: &PgPool,
    execution_id: &str,
    backoff_ms: i64,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE executions
         SET status = CASE WHEN attempt_count >= max_attempts THEN 'FAILED' ELSE 'RETRYING' END,
             run_at = CASE WHEN attempt_count >= max_attempts THEN run_at
                      ELSE now() + ($2 * interval '1 millisecond') END,
             worker_id = NULL,
             completed_at = CASE WHEN attempt_count >= max_attempts THEN now() ELSE NULL END,
             duration_ms = CASE WHEN attempt_count >= max_attempts
                           THEN (EXTRACT(EPOCH FROM (now() - started_at)) * 1000)::INT
                           ELSE NULL END
         WHERE execution_id = $1 AND status = 'RUNNING'"
    )
    .bind(execution_id)
    .bind(backoff_ms)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn complete_failed(
    pool: &PgPool,
    execution_id: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE executions
         SET status = 'FAILED', completed_at = now(),
             duration_ms = (EXTRACT(EPOCH FROM (now() - started_at)) * 1000)::INT,
             worker_id = NULL
         WHERE execution_id = $1 AND status = 'RUNNING'"
    )
    .bind(execution_id)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn get(pool: &PgPool, execution_id: &str) -> Result<Option<Execution>, sqlx::Error> {
    sqlx::query_as::<_, Execution>("SELECT * FROM executions WHERE execution_id = $1")
        .bind(execution_id)
        .fetch_optional(pool)
        .await
}

pub async fn get_for_job(pool: &PgPool, job_id: &str) -> Result<Option<Execution>, sqlx::Error> {
    sqlx::query_as::<_, Execution>(
        "SELECT * FROM executions WHERE job_id = $1 ORDER BY created_at DESC LIMIT 1"
    )
    .bind(job_id)
    .fetch_optional(pool)
    .await
}

pub async fn list_for_job(pool: &PgPool, job_id: &str, cursor: Option<&str>, limit: i64) -> Result<Vec<Execution>, sqlx::Error> {
    match cursor {
        Some(c) => {
            sqlx::query_as::<_, Execution>(
                "SELECT * FROM executions
                 WHERE job_id = $1 AND created_at < (SELECT created_at FROM executions WHERE execution_id = $2)
                 ORDER BY created_at DESC LIMIT $3"
            )
            .bind(job_id)
            .bind(c)
            .bind(limit)
            .fetch_all(pool)
            .await
        }
        None => {
            sqlx::query_as::<_, Execution>(
                "SELECT * FROM executions WHERE job_id = $1 ORDER BY created_at DESC LIMIT $2"
            )
            .bind(job_id)
            .bind(limit)
            .fetch_all(pool)
            .await
        }
    }
}

pub async fn cancel(pool: &PgPool, execution_id: &str) -> Result<Option<Execution>, sqlx::Error> {
    sqlx::query_as::<_, Execution>(
        "UPDATE executions SET status = 'CANCELLED', completed_at = now()
         WHERE execution_id = $1 AND status IN ('PENDING', 'QUEUED')
         RETURNING *"
    )
    .bind(execution_id)
    .fetch_optional(pool)
    .await
}

pub async fn promote_pending(pool: &PgPool) -> Result<u64, sqlx::Error> {
    let result = sqlx::query(
        "UPDATE executions SET status = 'QUEUED'
         WHERE status = 'PENDING' AND run_at <= now()"
    )
    .execute(pool)
    .await?;
    Ok(result.rows_affected())
}

pub async fn reclaim_stuck(pool: &PgPool, timeout_secs: i64) -> Result<u64, sqlx::Error> {
    let result = sqlx::query(
        "UPDATE executions
         SET status = CASE WHEN attempt_count >= max_attempts THEN 'FAILED' ELSE 'RETRYING' END,
             worker_id = NULL, run_at = now()
         WHERE status = 'RUNNING' AND started_at < now() - ($1 * interval '1 second')"
    )
    .bind(timeout_secs)
    .execute(pool)
    .await?;
    Ok(result.rows_affected())
}

pub async fn create_cron_execution(
    pool: &PgPool,
    job_id: &str,
    endpoint: &str,
    endpoint_type: &str,
    idempotency_key: &str,
    input: Option<&serde_json::Value>,
    run_at: DateTime<Utc>,
    max_attempts: i64,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(
        "INSERT INTO executions (job_id, endpoint, endpoint_type, idempotency_key, status, input, run_at, max_attempts)
         VALUES ($1, $2, $3, $4, 'QUEUED', $5, $6, $7)
         ON CONFLICT (job_id, idempotency_key) WHERE idempotency_key IS NOT NULL DO NOTHING"
    )
    .bind(job_id)
    .bind(endpoint)
    .bind(endpoint_type)
    .bind(idempotency_key)
    .bind(input)
    .bind(run_at)
    .bind(max_attempts)
    .execute(pool)
    .await?;
    Ok(result.rows_affected() > 0)
}

pub async fn cancel_pending_for_job(pool: &PgPool, job_id: &str) -> Result<u64, sqlx::Error> {
    let result = sqlx::query(
        "UPDATE executions SET status = 'CANCELLED', completed_at = now()
         WHERE job_id = $1 AND status IN ('PENDING', 'QUEUED')"
    )
    .bind(job_id)
    .execute(pool)
    .await?;
    Ok(result.rows_affected())
}
