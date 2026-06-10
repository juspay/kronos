use crate::{db::{tbl, DbContext}, models::job::Job};
use chrono::{DateTime, Utc};
use sqlx::{PgConnection, PgPool};

pub struct CreateJobResult {
    pub job: Job,
    pub execution_id: String,
    pub execution_status: String,
    pub execution_created_at: DateTime<Utc>,
}

pub async fn create_immediate(
    db: &mut DbContext<'_>,
    endpoint: &str,
    endpoint_type: &str,
    idempotency_key: &str,
    input: Option<&serde_json::Value>,
    max_attempts: i64,
) -> Result<CreateJobResult, sqlx::Error> {
    let tj = tbl(db.prefix, "jobs");
    let te = tbl(db.prefix, "executions");

    let job = sqlx::query_as::<_, Job>(&format!(
        "INSERT INTO {tj} (endpoint, endpoint_type, trigger_type, idempotency_key, input)
         VALUES ($1, $2, 'IMMEDIATE', $3, $4)
         RETURNING *"
    ))
    .bind(endpoint)
    .bind(endpoint_type)
    .bind(idempotency_key)
    .bind(input)
    .fetch_one(&mut *db.conn)
    .await?;

    let exec_row: (String, String, DateTime<Utc>) = sqlx::query_as(&format!(
        "INSERT INTO {te} (job_id, endpoint, endpoint_type, idempotency_key, status, run_at, input, max_attempts)
         VALUES ($1, $2, $3, $4, 'QUEUED', now(), $5, $6)
         RETURNING execution_id, status, created_at"
    ))
    .bind(&job.job_id)
    .bind(endpoint)
    .bind(endpoint_type)
    .bind(idempotency_key)
    .bind(input)
    .bind(max_attempts)
    .fetch_one(&mut *db.conn)
    .await?;

    Ok(CreateJobResult {
        job,
        execution_id: exec_row.0,
        execution_status: exec_row.1,
        execution_created_at: exec_row.2,
    })
}

pub async fn create_delayed(
    db: &mut DbContext<'_>,
    endpoint: &str,
    endpoint_type: &str,
    idempotency_key: &str,
    input: Option<&serde_json::Value>,
    run_at: DateTime<Utc>,
    max_attempts: i64,
) -> Result<CreateJobResult, sqlx::Error> {
    let tj = tbl(db.prefix, "jobs");
    let te = tbl(db.prefix, "executions");

    let job = sqlx::query_as::<_, Job>(&format!(
        "INSERT INTO {tj} (endpoint, endpoint_type, trigger_type, idempotency_key, input, run_at)
         VALUES ($1, $2, 'DELAYED', $3, $4, $5)
         RETURNING *"
    ))
    .bind(endpoint)
    .bind(endpoint_type)
    .bind(idempotency_key)
    .bind(input)
    .bind(run_at)
    .fetch_one(&mut *db.conn)
    .await?;

    let exec_row: (String, String, DateTime<Utc>) = sqlx::query_as(&format!(
        "INSERT INTO {te} (job_id, endpoint, endpoint_type, idempotency_key, status, run_at, input, max_attempts)
         VALUES ($1, $2, $3, $4, 'PENDING', $5, $6, $7)
         RETURNING execution_id, status, created_at"
    ))
    .bind(&job.job_id)
    .bind(endpoint)
    .bind(endpoint_type)
    .bind(idempotency_key)
    .bind(run_at)
    .bind(input)
    .bind(max_attempts)
    .fetch_one(&mut *db.conn)
    .await?;

    Ok(CreateJobResult {
        job,
        execution_id: exec_row.0,
        execution_status: exec_row.1,
        execution_created_at: exec_row.2,
    })
}

pub async fn create_cron(
    db: &mut DbContext<'_>,
    endpoint: &str,
    endpoint_type: &str,
    input: Option<&serde_json::Value>,
    cron_expression: &str,
    cron_timezone: &str,
    starts_at: Option<DateTime<Utc>>,
    ends_at: Option<DateTime<Utc>>,
    next_run_at: DateTime<Utc>,
) -> Result<Job, sqlx::Error> {
    let tj = tbl(db.prefix, "jobs");
    sqlx::query_as::<_, Job>(&format!(
        "INSERT INTO {tj} (endpoint, endpoint_type, trigger_type, input, cron_expression, cron_timezone, cron_starts_at, cron_ends_at, cron_next_run_at)
         VALUES ($1, $2, 'CRON', $3, $4, $5, $6, $7, $8)
         RETURNING *"
    ))
    .bind(endpoint)
    .bind(endpoint_type)
    .bind(input)
    .bind(cron_expression)
    .bind(cron_timezone)
    .bind(starts_at)
    .bind(ends_at)
    .bind(next_run_at)
    .fetch_one(&mut *db.conn)
    .await
}

pub async fn get(
    db: &mut DbContext<'_>,
    job_id: &str,
) -> Result<Option<Job>, sqlx::Error> {
    let t = tbl(db.prefix, "jobs");
    sqlx::query_as::<_, Job>(&format!("SELECT * FROM {t} WHERE job_id = $1"))
        .bind(job_id)
        .fetch_optional(&mut *db.conn)
        .await
}

pub async fn get_by_idempotency(
    db: &mut DbContext<'_>,
    endpoint: &str,
    key: &str,
) -> Result<Option<Job>, sqlx::Error> {
    let t = tbl(db.prefix, "jobs");
    sqlx::query_as::<_, Job>(&format!(
        "SELECT * FROM {t} WHERE endpoint = $1 AND idempotency_key = $2"
    ))
    .bind(endpoint)
    .bind(key)
    .fetch_optional(&mut *db.conn)
    .await
}

/// Optional, server-side filters applied to [`list`]. All fields are ANDed
/// together; `None` fields are simply not constrained. `endpoint` is matched as
/// a case-insensitive substring so the dashboard can offer a search box, while
/// the enum-like fields (`status`, `trigger`, `endpoint_type`) match exactly.
#[derive(Debug, Default, Clone)]
pub struct JobFilters {
    pub status: Option<String>,
    pub trigger: Option<String>,
    pub endpoint: Option<String>,
    pub endpoint_type: Option<String>,
}

/// Builds the `list` query and the ordered list of string binds. Kept pure (no
/// DB access) so the placeholder/bind bookkeeping can be unit tested. The final
/// `LIMIT` placeholder is left for the caller to bind as an `i64`.
fn build_list_query(t: &str, cursor: Option<&str>, filters: &JobFilters) -> (String, Vec<String>) {
    let mut conditions: Vec<String> = Vec::new();
    let mut binds: Vec<String> = Vec::new();
    let mut n = 1;

    if let Some(c) = cursor {
        conditions.push(format!(
            "created_at < (SELECT created_at FROM {t} WHERE job_id = ${n})"
        ));
        binds.push(c.to_string());
        n += 1;
    }
    if let Some(status) = &filters.status {
        conditions.push(format!("status = ${n}"));
        binds.push(status.clone());
        n += 1;
    }
    if let Some(trigger) = &filters.trigger {
        conditions.push(format!("trigger_type = ${n}"));
        binds.push(trigger.clone());
        n += 1;
    }
    if let Some(endpoint_type) = &filters.endpoint_type {
        conditions.push(format!("endpoint_type = ${n}"));
        binds.push(endpoint_type.clone());
        n += 1;
    }
    if let Some(endpoint) = &filters.endpoint {
        conditions.push(format!("endpoint ILIKE '%' || ${n} || '%'"));
        binds.push(endpoint.clone());
        n += 1;
    }

    let where_clause = if conditions.is_empty() {
        String::new()
    } else {
        format!(" WHERE {}", conditions.join(" AND "))
    };

    let sql = format!("SELECT * FROM {t}{where_clause} ORDER BY created_at DESC LIMIT ${n}");
    (sql, binds)
}

pub async fn list(
    db: &mut DbContext<'_>,
    cursor: Option<&str>,
    limit: i64,
    filters: &JobFilters,
) -> Result<Vec<Job>, sqlx::Error> {
    let t = tbl(db.prefix, "jobs");
    let (sql, binds) = build_list_query(&t, cursor, filters);
    let mut query = sqlx::query_as::<_, Job>(&sql);
    for bind in &binds {
        query = query.bind(bind);
    }
    query.bind(limit).fetch_all(&mut *db.conn).await
}

pub async fn cancel(
    db: &mut DbContext<'_>,
    job_id: &str,
) -> Result<Option<Job>, sqlx::Error> {
    let t = tbl(db.prefix, "jobs");
    sqlx::query_as::<_, Job>(&format!(
        "UPDATE {t} SET status = 'RETIRED', retired_at = now()
         WHERE job_id = $1 AND status = 'ACTIVE'
         RETURNING *"
    ))
    .bind(job_id)
    .fetch_optional(&mut *db.conn)
    .await
}

pub async fn retire_and_replace(
    db: &mut DbContext<'_>,
    old_job_id: &str,
    new_job: &Job,
) -> Result<Job, sqlx::Error> {
    let t = tbl(db.prefix, "jobs");
    sqlx::query(&format!(
        "UPDATE {t} SET status = 'RETIRED', retired_at = now(), replaced_by_id = $2
         WHERE job_id = $1 AND status = 'ACTIVE'"
    ))
    .bind(old_job_id)
    .bind(&new_job.job_id)
    .execute(&mut *db.conn)
    .await?;

    let new = sqlx::query_as::<_, Job>(&format!(
        "INSERT INTO {t} (endpoint, endpoint_type, trigger_type, input, cron_expression, cron_timezone, cron_starts_at, cron_ends_at, cron_next_run_at, version, previous_version_id)
         VALUES ($1, $2, 'CRON', $3, $4, $5, $6, $7, $8, $9, $10)
         RETURNING *"
    ))
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
    .fetch_one(&mut *db.conn)
    .await?;

    Ok(new)
}

pub async fn get_versions(
    db: &mut DbContext<'_>,
    job_id: &str,
) -> Result<Vec<Job>, sqlx::Error> {
    let t = tbl(db.prefix, "jobs");
    sqlx::query_as::<_, Job>(&format!(
        "WITH RECURSIVE chain AS (
            SELECT * FROM {t} WHERE job_id = $1
            UNION ALL
            SELECT j.* FROM {t} j JOIN chain c ON j.job_id = c.previous_version_id
         )
         SELECT * FROM chain ORDER BY version ASC"
    ))
    .bind(job_id)
    .fetch_all(&mut *db.conn)
    .await
}

/// Retire all CRON jobs whose `cron_ends_at` window has passed, returning the
/// `job_id`s that were retired so callers can unschedule their pg_cron entries.
///
/// This is the lifecycle counterpart to the `cron_ends_at` guard baked into the
/// pg_cron command (see [`build_cron_command`]): the guard stops *materializing*
/// executions past `ends_at`, while this reaps the now-dormant job so it flips to
/// RETIRED and its pg_cron entry can be removed instead of firing no-op inserts
/// forever.
pub async fn retire_expired_cron_jobs(
    conn: &mut PgConnection,
    prefix: &str,
) -> Result<Vec<String>, sqlx::Error> {
    let tj = tbl(prefix, "jobs");
    let rows: Vec<(String,)> = sqlx::query_as(&format!(
        "UPDATE {tj} SET status = 'RETIRED', retired_at = now()
         WHERE trigger_type = 'CRON' AND status = 'ACTIVE'
           AND cron_ends_at IS NOT NULL AND cron_ends_at <= now()
         RETURNING job_id"
    ))
    .fetch_all(&mut *conn)
    .await?;
    Ok(rows.into_iter().map(|r| r.0).collect())
}

/// Build the SQL command pg_cron runs on each tick for a CRON job.
///
/// Materializes a QUEUED execution by joining `jobs` with `endpoints`. The
/// `cron_ends_at` guard is critical: without it pg_cron keeps inserting
/// executions every tick forever, ignoring the job's end window. Table names are
/// resolved through `tbl(prefix, ..)` and schema-qualified because pg_cron runs
/// this command outside any scoped connection (no search_path).
fn build_cron_command(prefix: &str, schema_name: &str, job_id: &str) -> String {
    let te = tbl(prefix, "executions");
    let tj = tbl(prefix, "jobs");
    let tend = tbl(prefix, "endpoints");
    format!(
        "INSERT INTO \"{schema}\".\"{te}\" \
            (job_id, endpoint, endpoint_type, idempotency_key, status, input, run_at, max_attempts) \
         SELECT j.job_id, j.endpoint, j.endpoint_type, \
                'cron_' || j.job_id || '_' || (EXTRACT(EPOCH FROM now()) * 1000)::BIGINT, \
                'QUEUED', j.input, now(), \
                COALESCE((e.retry_policy->>'max_attempts')::BIGINT, 1) \
         FROM \"{schema}\".\"{tj}\" j \
         JOIN \"{schema}\".\"{tend}\" e ON e.name = j.endpoint \
         WHERE j.job_id = '{job_id}' AND j.status = 'ACTIVE' \
           AND (j.cron_ends_at IS NULL OR j.cron_ends_at > now()) \
         ON CONFLICT (job_id, idempotency_key) WHERE idempotency_key IS NOT NULL DO NOTHING",
        schema = schema_name,
        te = te,
        tj = tj,
        tend = tend,
        job_id = job_id,
    )
}


/// Register a CRON job with pg_cron. Uses the pool directly (pg_cron schedules
/// run outside a transaction) and takes prefix/schema_name explicitly.
pub async fn register_pg_cron(
    pool: &PgPool,
    prefix: &str,
    schema_name: &str,
    job_id: &str,
    cron_expression: &str,
) -> Result<(), sqlx::Error> {
    let cron_job_name = format!("kronos_{}_{}", schema_name, job_id);
    let command = build_cron_command(prefix, schema_name, job_id);

    sqlx::query("SELECT cron.schedule($1, $2, $3)")
        .bind(&cron_job_name)
        .bind(cron_expression)
        .bind(&command)
        .execute(pool)
        .await?;

    Ok(())
}

/// Register a CRON job with pg_cron on an existing connection (e.g. inside a
/// transaction), so the registration can commit atomically with the surrounding
/// row inserts. Mirrors [`unschedule_pg_cron_conn`] on the inverse path.
///
/// `cron.schedule` upserts by job name, so a caller that re-runs this against
/// the same `(schema_name, job_id)` simply replaces the previous command in
/// place — no manual existence check needed. A genuine failure (bad cron
/// expression, missing pg_cron extension) propagates, letting the caller roll
/// back the surrounding transaction.
pub async fn register_pg_cron_conn(
    conn: &mut PgConnection,
    prefix: &str,
    schema_name: &str,
    job_id: &str,
    cron_expression: &str,
) -> Result<(), sqlx::Error> {
    let cron_job_name = format!("kronos_{}_{}", schema_name, job_id);
    let command = build_cron_command(prefix, schema_name, job_id);

    sqlx::query("SELECT cron.schedule($1, $2, $3)")
        .bind(&cron_job_name)
        .bind(cron_expression)
        .bind(&command)
        .execute(&mut *conn)
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

/// Unschedule a CRON job's pg_cron entry on an existing connection (e.g. inside a
/// transaction), so it can be made atomic with another change such as retirement.
///
/// Existence-guarded via `WHERE jobname = $1`: a missing entry is a no-op rather
/// than an error (plain `cron.unschedule(name)` raises when the entry is absent).
/// A genuine failure still propagates, so a caller can roll back and retry.
pub async fn unschedule_pg_cron_conn(
    conn: &mut PgConnection,
    schema_name: &str,
    job_id: &str,
) -> Result<(), sqlx::Error> {
    let cron_job_name = format!("kronos_{}_{}", schema_name, job_id);

    sqlx::query("SELECT cron.unschedule(jobid) FROM cron.job WHERE jobname = $1")
        .bind(&cron_job_name)
        .execute(&mut *conn)
        .await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cron_command_enforces_ends_at_window() {
        let cmd = build_cron_command("", "ws_acme", "job-123");
        // The guard is the whole point of the fix: without it pg_cron keeps
        // materializing executions forever, past the job's end window.
        assert!(
            cmd.contains("j.cron_ends_at IS NULL OR j.cron_ends_at > now()"),
            "pg_cron command must guard on cron_ends_at, got: {cmd}"
        );
    }

    #[test]
    fn cron_command_targets_correct_schema_and_job() {
        let cmd = build_cron_command("", "ws_acme", "job-123");
        assert!(cmd.contains("\"ws_acme\".\"executions\""));
        assert!(cmd.contains("\"ws_acme\".\"jobs\" j"));
        assert!(cmd.contains("j.job_id = 'job-123'"));
        // Only active jobs should materialize.
        assert!(cmd.contains("j.status = 'ACTIVE'"));
    }

    #[test]
    fn cron_command_applies_table_prefix() {
        // Library mode: a non-empty prefix produces prefixed table names within
        // the schema (tbl(prefix, ..)), matching the rest of the DB layer.
        let cmd = build_cron_command("sched", "ws_acme", "job-123");
        assert!(cmd.contains("\"ws_acme\".\"sched_executions\""));
        assert!(cmd.contains("\"ws_acme\".\"sched_jobs\" j"));
        assert!(cmd.contains("\"ws_acme\".\"sched_endpoints\" e"));
    }

    #[test]
    fn list_query_without_cursor_or_filters() {
        let (sql, binds) = build_list_query("jobs", None, &JobFilters::default());
        assert_eq!(sql, "SELECT * FROM jobs ORDER BY created_at DESC LIMIT $1");
        assert!(binds.is_empty());
    }

    #[test]
    fn list_query_with_cursor_only() {
        let (sql, binds) = build_list_query("jobs", Some("job-9"), &JobFilters::default());
        assert_eq!(
            sql,
            "SELECT * FROM jobs WHERE created_at < (SELECT created_at FROM jobs WHERE job_id = $1) \
             ORDER BY created_at DESC LIMIT $2"
        );
        assert_eq!(binds, vec!["job-9".to_string()]);
    }

    #[test]
    fn list_query_binds_filters_in_order_after_cursor() {
        let filters = JobFilters {
            status: Some("ACTIVE".into()),
            trigger: Some("CRON".into()),
            endpoint_type: Some("HTTP".into()),
            endpoint: Some("notify".into()),
        };
        let (sql, binds) = build_list_query("jobs", Some("job-9"), &filters);
        // Cursor is $1, filters follow in declaration order, LIMIT is last ($6).
        assert_eq!(
            sql,
            "SELECT * FROM jobs WHERE \
             created_at < (SELECT created_at FROM jobs WHERE job_id = $1) AND \
             status = $2 AND trigger_type = $3 AND endpoint_type = $4 AND \
             endpoint ILIKE '%' || $5 || '%' \
             ORDER BY created_at DESC LIMIT $6"
        );
        assert_eq!(
            binds,
            vec![
                "job-9".to_string(),
                "ACTIVE".to_string(),
                "CRON".to_string(),
                "HTTP".to_string(),
                "notify".to_string(),
            ]
        );
    }

    #[test]
    fn list_query_filters_without_cursor_start_at_one() {
        let filters = JobFilters {
            status: Some("RETIRED".into()),
            ..Default::default()
        };
        let (sql, binds) = build_list_query("jobs", None, &filters);
        assert_eq!(
            sql,
            "SELECT * FROM jobs WHERE status = $1 ORDER BY created_at DESC LIMIT $2"
        );
        assert_eq!(binds, vec!["RETIRED".to_string()]);
    }
}
