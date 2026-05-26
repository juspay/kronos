//! CRON job reaper.
//!
//! pg_cron drives execution materialization, but it has no concept of a job's
//! `cron_ends_at` window — left alone it fires forever. The `cron_ends_at` guard
//! in the pg_cron command stops *new executions* past the window; this reaper
//! handles the *lifecycle*: it periodically flips expired CRON jobs to RETIRED
//! and removes their pg_cron entry so they stop firing no-op inserts entirely.
//!
//! It runs as a lightweight background task alongside the poller. The work is
//! idempotent: each sweep only retires jobs still `ACTIVE` past their end window,
//! so a missed sweep simply gets picked up on the next tick.

use kronos_common::{config::AppConfig, db, metrics as m, tenant::SchemaRegistry};
use sqlx::PgPool;
use std::time::Duration;

/// Run the reaper loop until the process exits. Sweeps every
/// `worker.reaper_interval_sec` seconds across all active workspace schemas.
pub async fn run(pool: PgPool, config: AppConfig) -> anyhow::Result<()> {
    let interval = Duration::from_secs(config.worker.reaper_interval_sec);
    let schema_registry = SchemaRegistry::new(pool.clone(), 30);

    tracing::info!(
        interval_sec = config.worker.reaper_interval_sec,
        "CRON reaper started"
    );

    loop {
        tokio::time::sleep(interval).await;

        let schemas = match schema_registry.get_active_schemas().await {
            Ok(s) => s,
            Err(e) => {
                tracing::error!("Reaper failed to fetch active schemas: {}", e);
                continue;
            }
        };

        for schema_name in &schemas {
            if let Err(e) = reap_schema(&pool, schema_name).await {
                tracing::error!(schema = %schema_name, "Reaper sweep failed: {}", e);
            }
        }
    }
}

/// Retire expired CRON jobs in a single schema and unschedule their pg_cron
/// entries — both in one transaction so they are atomic.
///
/// Retiring and unscheduling together means a failed unschedule rolls the whole
/// batch back, to be retried on the next sweep; we never commit a RETIRED job
/// while leaving its pg_cron entry scheduled forever (a permanent leak, since
/// future sweeps only look at ACTIVE jobs). The unschedule is existence-guarded,
/// so an already-removed entry is a no-op rather than an error.
async fn reap_schema(pool: &PgPool, schema_name: &str) -> anyhow::Result<()> {
    let mut tx = db::scoped::scoped_transaction(pool, schema_name).await?;
    let retired = db::jobs::retire_expired_cron_jobs(&mut *tx).await?;

    for job_id in &retired {
        db::jobs::unschedule_pg_cron_conn(&mut *tx, schema_name, job_id).await?;
    }

    tx.commit().await?;

    if retired.is_empty() {
        return Ok(());
    }

    metrics::counter!(m::CRON_JOBS_REAPED_TOTAL, "schema" => schema_name.to_string())
        .increment(retired.len() as u64);
    for job_id in &retired {
        tracing::info!(schema = %schema_name, job_id = %job_id, "Reaped expired CRON job");
    }

    Ok(())
}
