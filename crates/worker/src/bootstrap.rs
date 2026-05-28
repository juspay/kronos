//! Provision and maintain kronos's own dogfooded internal jobs.
//!
//! Today there's exactly one: the per-schema CRON reaper. For each active
//! workspace schema we ensure (a) an `INTERNAL` endpoint named `kronos.reaper`
//! exists and (b) a `CRON` job (`* * * * *`) is registered against it, both in
//! the workspace's own schema and in pg_cron. Each tick then materializes an
//! execution into the workspace's `executions` table — picked up by the worker
//! pool, routed to the internal dispatcher, and visible in the dashboard like
//! any user job.
//!
//! Runs as a lightweight tokio loop: every pass walks the active workspaces
//! and ensures the endpoint, job, and pg_cron entry all exist. SchemaRegistry
//! refreshes every 30s, so a freshly-created workspace gets its reaper job
//! within roughly one bootstrap interval. Every step is idempotent — ON
//! CONFLICT on the endpoint and on the job's `(endpoint, idempotency_key)`
//! unique index, and `cron.schedule` upserts by name — so concurrent worker
//! pods racing through bootstrap don't conflict, and the first pass after a
//! fresh install does the same provisioning work as a steady-state pass.

use kronos_common::{config::AppConfig, db, tenant::SchemaRegistry};
use sqlx::PgPool;
use std::time::Duration;

/// Fixed identifiers for the reaper's INTERNAL endpoint and CRON job. Kept in
/// one place so the bootstrap, the migration, and any future ops tooling all
/// agree on the names.
const REAPER_ENDPOINT_NAME: &str = "kronos.reaper";
/// Pinned `idempotency_key` for the reaper job. Lands on the partial unique
/// index `(endpoint, idempotency_key) WHERE idempotency_key IS NOT NULL`, so
/// `INSERT ... ON CONFLICT DO NOTHING` is a clean no-op on races.
const REAPER_JOB_IDEMPOTENCY_KEY: &str = "kronos.reaper";
/// pg_cron's 5-field expressions max out at minute granularity, so the reaper
/// runs once a minute. Plenty for a lifecycle sweep — the `cron_ends_at` guard
/// inside `build_cron_command` already stops *new* executions immediately.
const REAPER_CRON_EXPRESSION: &str = "* * * * *";

/// Run the bootstrap loop until the process exits. Refreshes per-schema state
/// every `interval`, idempotently — first pass usually has work, subsequent
/// passes are no-ops unless a new workspace appeared.
pub async fn run(pool: PgPool, _config: AppConfig) -> anyhow::Result<()> {
    let interval = Duration::from_secs(60);
    let schema_registry = SchemaRegistry::new(pool.clone(), 30);

    tracing::info!(
        interval_sec = interval.as_secs(),
        "Worker bootstrap loop started (provisions kronos.reaper INTERNAL CRON job per workspace)"
    );

    loop {
        if let Err(e) = bootstrap_once(&pool, &schema_registry).await {
            tracing::error!("Worker bootstrap pass failed: {}", e);
        }
        tokio::time::sleep(interval).await;
    }
}

/// Single bootstrap pass over every active workspace schema.
async fn bootstrap_once(pool: &PgPool, schema_registry: &SchemaRegistry) -> anyhow::Result<()> {
    let schemas = schema_registry.get_active_schemas().await?;

    for schema_name in &schemas {
        if let Err(e) = ensure_reaper_job(pool, schema_name).await {
            tracing::error!(
                schema = %schema_name,
                "Failed to bootstrap reaper job: {}", e
            );
        }
    }

    Ok(())
}

/// Ensure the `kronos.reaper` INTERNAL endpoint, its CRON job, and the matching
/// pg_cron entry all exist for `schema_name`. Each step is independently
/// idempotent so partial state from a crashed earlier pass converges on the
/// next.
async fn ensure_reaper_job(pool: &PgPool, schema_name: &str) -> anyhow::Result<()> {
    let mut tx = db::scoped::scoped_transaction(pool, schema_name).await?;

    sqlx::query(
        "INSERT INTO endpoints (name, endpoint_type, spec) \
         VALUES ($1, 'INTERNAL', '{\"task\":\"reaper\"}'::jsonb) \
         ON CONFLICT (name) DO NOTHING",
    )
    .bind(REAPER_ENDPOINT_NAME)
    .execute(&mut *tx)
    .await?;

    // ON CONFLICT DO NOTHING on the partial unique index handles pod races: only
    // one of the racing inserts gets a row back; the others see `None` and rely
    // on the follow-up SELECT to recover the existing job_id.
    let inserted: Option<(String,)> = sqlx::query_as(
        "INSERT INTO jobs ( \
            endpoint, endpoint_type, trigger_type, idempotency_key, \
            cron_expression, cron_timezone, cron_next_run_at \
         ) VALUES ($1, 'INTERNAL', 'CRON', $2, $3, 'UTC', now()) \
         ON CONFLICT (endpoint, idempotency_key) WHERE idempotency_key IS NOT NULL DO NOTHING \
         RETURNING job_id",
    )
    .bind(REAPER_ENDPOINT_NAME)
    .bind(REAPER_JOB_IDEMPOTENCY_KEY)
    .bind(REAPER_CRON_EXPRESSION)
    .fetch_optional(&mut *tx)
    .await?;

    let job_id = match inserted {
        Some((id,)) => id,
        None => {
            let existing: Option<(String,)> = sqlx::query_as(
                "SELECT job_id FROM jobs \
                 WHERE endpoint = $1 AND idempotency_key = $2 AND status = 'ACTIVE' \
                 LIMIT 1",
            )
            .bind(REAPER_ENDPOINT_NAME)
            .bind(REAPER_JOB_IDEMPOTENCY_KEY)
            .fetch_optional(&mut *tx)
            .await?;

            match existing {
                Some((id,)) => id,
                None => {
                    // No active reaper job — likely retired by an operator.
                    // Skip pg_cron registration and try again next pass.
                    tx.commit().await?;
                    return Ok(());
                }
            }
        }
    };

    tx.commit().await?;

    // pg_cron registration happens outside the workspace tx: cron.schedule
    // writes to the `cron` schema and upserts by name, so re-running is a
    // harmless replace-in-place even when multiple pods race.
    db::jobs::register_pg_cron(pool, schema_name, &job_id, REAPER_CRON_EXPRESSION).await?;

    Ok(())
}
