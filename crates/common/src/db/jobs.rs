use crate::models::job::Job;
use chrono::{DateTime, Utc};
use sqlx::{PgConnection, PgPool};

pub struct CreateJobResult {
    pub job: Job,
    pub execution_id: String,
    pub execution_status: String,
    pub execution_created_at: DateTime<Utc>,
}

pub async fn create_immediate(
    conn: &mut PgConnection,
    endpoint: &str,
    endpoint_type: &str,
    idempotency_key: &str,
    input: Option<&serde_json::Value>,
    max_attempts: i64,
) -> Result<CreateJobResult, sqlx::Error> {
    let job = sqlx::query_as::<_, Job>(
        "INSERT INTO jobs (endpoint, endpoint_type, trigger_type, idempotency_key, input)
         VALUES ($1, $2, 'IMMEDIATE', $3, $4)
         RETURNING *",
    )
    .bind(endpoint)
    .bind(endpoint_type)
    .bind(idempotency_key)
    .bind(input)
    .fetch_one(&mut *conn)
    .await?;

    let exec_row: (String, String, DateTime<Utc>) = sqlx::query_as(
        "INSERT INTO executions (job_id, endpoint, endpoint_type, idempotency_key, status, run_at, input, max_attempts)
         VALUES ($1, $2, $3, $4, 'QUEUED', now(), $5, $6)
         RETURNING execution_id, status, created_at"
    )
    .bind(&job.job_id)
    .bind(endpoint)
    .bind(endpoint_type)
    .bind(idempotency_key)
    .bind(input)
    .bind(max_attempts)
    .fetch_one(&mut *conn)
    .await?;

    Ok(CreateJobResult {
        job,
        execution_id: exec_row.0,
        execution_status: exec_row.1,
        execution_created_at: exec_row.2,
    })
}

pub async fn create_delayed(
    conn: &mut PgConnection,
    endpoint: &str,
    endpoint_type: &str,
    idempotency_key: &str,
    input: Option<&serde_json::Value>,
    run_at: DateTime<Utc>,
    max_attempts: i64,
) -> Result<CreateJobResult, sqlx::Error> {
    let job = sqlx::query_as::<_, Job>(
        "INSERT INTO jobs (endpoint, endpoint_type, trigger_type, idempotency_key, input, run_at)
         VALUES ($1, $2, 'DELAYED', $3, $4, $5)
         RETURNING *",
    )
    .bind(endpoint)
    .bind(endpoint_type)
    .bind(idempotency_key)
    .bind(input)
    .bind(run_at)
    .fetch_one(&mut *conn)
    .await?;

    let exec_row: (String, String, DateTime<Utc>) = sqlx::query_as(
        "INSERT INTO executions (job_id, endpoint, endpoint_type, idempotency_key, status, run_at, input, max_attempts)
         VALUES ($1, $2, $3, $4, 'PENDING', $5, $6, $7)
         RETURNING execution_id, status, created_at"
    )
    .bind(&job.job_id)
    .bind(endpoint)
    .bind(endpoint_type)
    .bind(idempotency_key)
    .bind(run_at)
    .bind(input)
    .bind(max_attempts)
    .fetch_one(&mut *conn)
    .await?;

    Ok(CreateJobResult {
        job,
        execution_id: exec_row.0,
        execution_status: exec_row.1,
        execution_created_at: exec_row.2,
    })
}

pub async fn create_cron(
    conn: &mut PgConnection,
    endpoint: &str,
    endpoint_type: &str,
    input: Option<&serde_json::Value>,
    cron_expression: &str,
    cron_timezone: &str,
    starts_at: Option<DateTime<Utc>>,
    ends_at: Option<DateTime<Utc>>,
    next_run_at: DateTime<Utc>,
) -> Result<Job, sqlx::Error> {
    sqlx::query_as::<_, Job>(
        "INSERT INTO jobs (endpoint, endpoint_type, trigger_type, input, cron_expression, cron_timezone, cron_starts_at, cron_ends_at, cron_next_run_at)
         VALUES ($1, $2, 'CRON', $3, $4, $5, $6, $7, $8)
         RETURNING *"
    )
    .bind(endpoint)
    .bind(endpoint_type)
    .bind(input)
    .bind(cron_expression)
    .bind(cron_timezone)
    .bind(starts_at)
    .bind(ends_at)
    .bind(next_run_at)
    .fetch_one(&mut *conn)
    .await
}

pub async fn get(conn: &mut PgConnection, job_id: &str) -> Result<Option<Job>, sqlx::Error> {
    sqlx::query_as::<_, Job>("SELECT * FROM jobs WHERE job_id = $1")
        .bind(job_id)
        .fetch_optional(&mut *conn)
        .await
}

pub async fn get_by_idempotency(
    conn: &mut PgConnection,
    endpoint: &str,
    key: &str,
) -> Result<Option<Job>, sqlx::Error> {
    sqlx::query_as::<_, Job>("SELECT * FROM jobs WHERE endpoint = $1 AND idempotency_key = $2")
        .bind(endpoint)
        .bind(key)
        .fetch_optional(&mut *conn)
        .await
}

pub async fn list(
    conn: &mut PgConnection,
    cursor: Option<&str>,
    limit: i64,
) -> Result<Vec<Job>, sqlx::Error> {
    match cursor {
        Some(c) => sqlx::query_as::<_, Job>(
            "SELECT * FROM jobs WHERE created_at < (SELECT created_at FROM jobs WHERE job_id = $1)
                 ORDER BY created_at DESC LIMIT $2",
        )
        .bind(c)
        .bind(limit)
        .fetch_all(&mut *conn)
        .await,
        None => {
            sqlx::query_as::<_, Job>("SELECT * FROM jobs ORDER BY created_at DESC LIMIT $1")
                .bind(limit)
                .fetch_all(&mut *conn)
                .await
        }
    }
}

pub async fn cancel(conn: &mut PgConnection, job_id: &str) -> Result<Option<Job>, sqlx::Error> {
    sqlx::query_as::<_, Job>(
        "UPDATE jobs SET status = 'RETIRED', retired_at = now()
         WHERE job_id = $1 AND status = 'ACTIVE'
         RETURNING *",
    )
    .bind(job_id)
    .fetch_optional(&mut *conn)
    .await
}

pub async fn retire_and_replace(
    conn: &mut PgConnection,
    old_job_id: &str,
    new_job: &Job,
) -> Result<Job, sqlx::Error> {
    sqlx::query(
        "UPDATE jobs SET status = 'RETIRED', retired_at = now(), replaced_by_id = $2
         WHERE job_id = $1 AND status = 'ACTIVE'",
    )
    .bind(old_job_id)
    .bind(&new_job.job_id)
    .execute(&mut *conn)
    .await?;

    let new = sqlx::query_as::<_, Job>(
        "INSERT INTO jobs (endpoint, endpoint_type, trigger_type, input, cron_expression, cron_timezone, cron_starts_at, cron_ends_at, cron_next_run_at, version, previous_version_id)
         VALUES ($1, $2, 'CRON', $3, $4, $5, $6, $7, $8, $9, $10)
         RETURNING *"
    )
    .bind(&new_job.endpoint)
    .bind(&new_job.endpoint_type)
    .bind(&new_job.input)
    .bind(&new_job.cron_expression)
    .bind(&new_job.cron_timezone)
    .bind(&new_job.cron_starts_at)
    .bind(&new_job.cron_ends_at)
    .bind(&new_job.cron_next_run_at)
    .bind(new_job.version)
    .bind(old_job_id)
    .fetch_one(&mut *conn)
    .await?;

    Ok(new)
}

pub async fn get_versions(conn: &mut PgConnection, job_id: &str) -> Result<Vec<Job>, sqlx::Error> {
    sqlx::query_as::<_, Job>(
        "WITH RECURSIVE chain AS (
            SELECT * FROM jobs WHERE job_id = $1
            UNION ALL
            SELECT j.* FROM jobs j JOIN chain c ON j.job_id = c.previous_version_id
         )
         SELECT * FROM chain ORDER BY version ASC",
    )
    .bind(job_id)
    .fetch_all(&mut *conn)
    .await
}

pub async fn get_due_cron_jobs(conn: &mut PgConnection, limit: i64) -> Result<Vec<Job>, sqlx::Error> {
    sqlx::query_as::<_, Job>(
        "SELECT * FROM jobs
         WHERE trigger_type = 'CRON' AND status = 'ACTIVE'
           AND cron_next_run_at <= now()
           AND (cron_ends_at IS NULL OR cron_ends_at > now())
         LIMIT $1",
    )
    .bind(limit)
    .fetch_all(&mut *conn)
    .await
}

pub async fn advance_cron_tick(
    conn: &mut PgConnection,
    job_id: &str,
    current_tick: DateTime<Utc>,
    next_tick: DateTime<Utc>,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(
        "UPDATE jobs SET cron_next_run_at = $2, cron_last_tick_at = $3
         WHERE job_id = $1 AND cron_next_run_at = $3",
    )
    .bind(job_id)
    .bind(next_tick)
    .bind(current_tick)
    .execute(&mut *conn)
    .await?;
    Ok(result.rows_affected() > 0)
}

/// Register a CRON job with pg_cron. The pg_cron job will directly INSERT
/// execution rows when the cron schedule fires.
pub async fn register_pg_cron(
    pool: &PgPool,
    schema_name: &str,
    job_id: &str,
    cron_expression: &str,
) -> Result<(), sqlx::Error> {
    let cron_job_name = format!("kronos_{}_{}", schema_name, job_id);

    // The SQL command that pg_cron will execute on each tick.
    // It creates a QUEUED execution by joining with the jobs and endpoints tables.
    let command = format!(
        "INSERT INTO \"{schema}\".executions \
            (job_id, endpoint, endpoint_type, idempotency_key, status, input, run_at, max_attempts) \
         SELECT j.job_id, j.endpoint, j.endpoint_type, \
                'cron_' || j.job_id || '_' || (EXTRACT(EPOCH FROM now()) * 1000)::BIGINT, \
                'QUEUED', j.input, now(), \
                COALESCE((e.retry_policy->>'max_attempts')::BIGINT, 1) \
         FROM \"{schema}\".jobs j \
         JOIN \"{schema}\".endpoints e ON e.name = j.endpoint \
         WHERE j.job_id = '{job_id}' AND j.status = 'ACTIVE' \
         ON CONFLICT (job_id, idempotency_key) WHERE idempotency_key IS NOT NULL DO NOTHING",
        schema = schema_name,
        job_id = job_id,
    );

    sqlx::query("SELECT cron.schedule($1, $2, $3)")
        .bind(&cron_job_name)
        .bind(cron_expression)
        .bind(&command)
        .execute(pool)
        .await?;

    Ok(())
}

/// Unregister a CRON job from pg_cron.
pub async fn unregister_pg_cron(
    pool: &PgPool,
    schema_name: &str,
    job_id: &str,
) -> Result<(), sqlx::Error> {
    let cron_job_name = format!("kronos_{}_{}", schema_name, job_id);

    sqlx::query("SELECT cron.unschedule($1)")
        .bind(&cron_job_name)
        .execute(pool)
        .await?;

    Ok(())
}
