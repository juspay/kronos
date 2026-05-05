use kronos_common::{
    cache::{ConfigCache, SecretCache},
    config::AppConfig,
    db, metrics as m,
    tenant::SchemaProvider,
};
use reqwest::Client;
use sqlx::PgPool;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::pipeline::{self, PipelineContext};

pub async fn run<S: SchemaProvider>(
    pool: PgPool,
    config: AppConfig,
    schema_provider: S,
    cancel: CancellationToken,
) -> anyhow::Result<()> {
    let worker_id = format!("worker_{}", Uuid::new_v4().simple());
    let semaphore = Arc::new(Semaphore::new(config.worker.max_concurrent));
    let poll_interval = Duration::from_millis(config.worker.poll_interval_ms);
    let schema_provider = Arc::new(schema_provider);

    let ctx = Arc::new(PipelineContext {
        pool: pool.clone(),
        http_client: Client::new(),
        config_cache: ConfigCache::new(config.worker.config_cache_ttl_sec),
        secret_cache: SecretCache::new(config.worker.secret_cache_ttl_sec),
        encryption_key: config.crypto.encryption_key.clone(),
        table_prefix: config.db.table_prefix.clone(),
    });

    tracing::info!(worker_id = %worker_id, "Worker polling started");

    let idle = Arc::new(AtomicBool::new(false));

    // Single-threaded poller loop with bounded concurrency.
    // The semaphore (max_concurrent permits) gates how many tasks run in parallel.
    // The loop spins freely while permits are available, only sleeping when the
    // previous iteration found no work (idle backoff). Each spawned task holds a
    // permit and releases it on completion, unblocking the next iteration.
    loop {
        if idle.load(Ordering::Relaxed) {
            tokio::select! {
                _ = tokio::time::sleep(poll_interval) => {
                    idle.store(false, Ordering::Relaxed);
                }
                _ = cancel.cancelled() => {
                    break;
                }
            }
        }

        tokio::select! {
            _ = cancel.cancelled() => {
                break;
            }
            permit = semaphore.clone().acquire_owned() => {
                let permit = permit?;

                let schemas = match schema_provider.get_active_schemas().await {
                    Ok(s) => s,
                    Err(e) => {
                        tracing::error!("Failed to fetch active schemas: {}", e);
                        drop(permit);
                        tokio::select! {
                            _ = tokio::time::sleep(poll_interval) => {}
                            _ = cancel.cancelled() => { break; }
                        }
                        continue;
                    }
                };

                let pool = pool.clone();
                let ctx = ctx.clone();
                let wid = worker_id.clone();
                let idle = idle.clone();

                tokio::spawn(async move {
                    let found = claim_and_process(&pool, &ctx, &schemas, &wid).await;
                    if !found {
                        metrics::counter!(m::WORKER_POLL_IDLE_TOTAL,
                            "worker_id" => wid,
                        )
                        .increment(1);
                        idle.store(true, Ordering::Relaxed);
                    }
                    drop(permit);
                });
            }
        }
    }

    tracing::info!("Shutting down worker, waiting for in-flight tasks...");
    let timeout = Duration::from_secs(config.worker.shutdown_timeout_sec);
    let _ = tokio::time::timeout(timeout, async {
        let _all = semaphore
            .acquire_many(config.worker.max_concurrent as u32)
            .await;
    })
    .await;
    tracing::info!("Worker shutdown complete");
    Ok(())
}

async fn claim_and_process(
    pool: &PgPool,
    ctx: &PipelineContext,
    schemas: &[String],
    worker_id: &str,
) -> bool {
    let prefix = ctx.table_prefix.as_str();

    for schema_name in schemas {
        let mut tx = match db::scoped::scoped_transaction(pool, schema_name).await {
            Ok(tx) => tx,
            Err(e) => {
                tracing::error!(schema = %schema_name, "Failed to begin scoped transaction: {}", e);
                continue;
            }
        };

        // Soft cron tick (no pg_cron): materialize due CRON executions before claiming.
        #[cfg(not(feature = "pg_cron"))]
        tick_cron_jobs(&mut *tx, ctx, schema_name).await;

        let exec = match db::executions::claim(&mut *tx, prefix, worker_id).await {
            Ok(Some(exec)) => exec,
            Ok(None) => continue,
            Err(e) => {
                tracing::error!(schema = %schema_name, "Failed to claim execution: {}", e);
                continue;
            }
        };

        let job = match db::jobs::get(&mut *tx, prefix, &exec.job_id).await {
            Ok(Some(job)) => job,
            Ok(None) => {
                tracing::error!(schema = %schema_name, "Associated job for execution {} not found", exec.execution_id);
                continue;
            }
            Err(e) => {
                tracing::error!(schema = %schema_name, "Failed to fetch associated job: {}", e);
                tracing::warn!(schema = %schema_name, "Marking execution as failed: {}", e);
                let _ =
                    db::executions::complete_failed(&mut *tx, prefix, &exec.execution_id).await;
                continue;
            }
        };

        metrics::counter!(m::EXECUTIONS_CLAIMED_TOTAL,
            "schema" => schema_name.clone(),
            "endpoint_type" => exec.endpoint_type.clone(),
        )
        .increment(1);

        metrics::gauge!(m::WORKER_INFLIGHT, "worker_id" => worker_id.to_string()).increment(1.0);

        let idempotency_key: &str = job
            .idempotency_key
            .as_ref()
            .map(|v| v.as_str())
            .unwrap_or(exec.execution_id.as_str());

        pipeline::process_execution(
            ctx,
            &mut *tx,
            schema_name,
            &exec.execution_id,
            idempotency_key,
            &exec.job_id,
            &exec.endpoint,
            &exec.endpoint_type,
            exec.input.as_ref(),
            exec.attempt_count,
            exec.max_attempts,
        )
        .await;

        if let Err(e) = tx.commit().await {
            tracing::error!(
                execution_id = %exec.execution_id,
                "Failed to commit transaction: {}", e
            );
        }

        metrics::gauge!(m::WORKER_INFLIGHT, "worker_id" => worker_id.to_string()).decrement(1.0);

        return true;
    }

    false
}

/// Materialize due CRON executions without pg_cron.
/// Each worker races to advance the tick; the unique index on (job_id, idempotency_key)
/// prevents duplicate executions, and the CAS in advance_cron_tick prevents double-advance.
#[cfg(not(feature = "pg_cron"))]
async fn tick_cron_jobs(
    conn: &mut sqlx::PgConnection,
    ctx: &PipelineContext,
    schema_name: &str,
) {
    let prefix = ctx.table_prefix.as_str();

    let due_jobs = match db::jobs::get_due_cron_jobs(conn, prefix, 20).await {
        Ok(jobs) => jobs,
        Err(e) => {
            tracing::warn!(schema = %schema_name, "Failed to fetch due cron jobs: {}", e);
            return;
        }
    };

    for job in due_jobs {
        let current_tick = match job.cron_next_run_at {
            Some(t) => t,
            None => continue,
        };

        let cron_expr = match job.cron_expression.as_deref() {
            Some(expr) => expr,
            None => continue,
        };

        let tz_str = job
            .cron_timezone
            .as_deref()
            .unwrap_or("UTC");

        let next_tick = match compute_next_cron_tick(cron_expr, tz_str, current_tick) {
            Some(t) => t,
            None => {
                tracing::warn!(
                    schema = %schema_name,
                    job_id = %job.job_id,
                    "Failed to compute next cron tick for expression: {}",
                    cron_expr
                );
                continue;
            }
        };

        let idempotency_key = format!(
            "cron_{}_{}",
            job.job_id,
            current_tick.timestamp_millis()
        );

        let max_attempts = ctx
            .config_cache
            .get("_noop") // just to access pool below
            .map(|_| 1i64)
            .unwrap_or(1i64);
        let _ = max_attempts; // resolved from endpoint below

        // Look up endpoint to get max_attempts from retry_policy
        let ep_max_attempts = match db::endpoints::get(conn, prefix, &job.endpoint).await {
            Ok(Some(ep)) => ep.get_retry_policy().max_attempts,
            _ => 1,
        };

        let _ = db::executions::create_cron_execution(
            conn,
            prefix,
            &job.job_id,
            &job.endpoint,
            &job.endpoint_type,
            &idempotency_key,
            job.input.as_ref(),
            current_tick,
            ep_max_attempts,
        )
        .await;

        let _ = db::jobs::advance_cron_tick(conn, prefix, &job.job_id, current_tick, next_tick).await;
    }
}

#[cfg(not(feature = "pg_cron"))]
fn compute_next_cron_tick(
    expression: &str,
    timezone: &str,
    after: chrono::DateTime<chrono::Utc>,
) -> Option<chrono::DateTime<chrono::Utc>> {
    use chrono_tz::Tz;
    use std::str::FromStr;

    // Build a 7-field cron expression from a 5-field pg_cron expression:
    // pg_cron: "min hour dom month dow" → cron crate: "sec min hour dom month dow year"
    let parts: Vec<&str> = expression.split_whitespace().collect();
    if parts.len() != 5 {
        return None;
    }
    let seven_field = format!("0 {} {} {} {} {} *", parts[0], parts[1], parts[2], parts[3], parts[4]);

    let schedule = cron::Schedule::from_str(&seven_field).ok()?;

    let tz: Tz = timezone.parse().unwrap_or(chrono_tz::UTC);
    let after_local = after.with_timezone(&tz);

    schedule
        .after(&after_local)
        .next()
        .map(|t| t.with_timezone(&chrono::Utc))
}
