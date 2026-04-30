use kronos_common::{
    cache::{ConfigCache, SecretCache},
    config::AppConfig,
    db, metrics as m,
    tenant::SchemaRegistry,
};
use reqwest::Client;
use sqlx::PgPool;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Semaphore;
use uuid::Uuid;

use crate::pipeline::{self, PipelineContext};

pub async fn run(pool: PgPool, config: AppConfig) -> anyhow::Result<()> {
    let worker_id = format!("worker_{}", Uuid::new_v4().simple());
    let semaphore = Arc::new(Semaphore::new(config.worker.max_concurrent));
    let poll_interval = Duration::from_millis(config.worker.poll_interval_ms);
    let schema_registry = SchemaRegistry::new(
        pool.clone(),
        config.schema.system_schema.clone(),
        30,
    );

    let ctx = Arc::new(PipelineContext {
        pool: pool.clone(),
        http_client: Client::new(),
        config_cache: ConfigCache::new(config.worker.config_cache_ttl_sec),
        secret_cache: SecretCache::new(config.worker.secret_cache_ttl_sec),
        encryption_key: config.crypto.encryption_key.clone(),
    });

    tracing::info!(worker_id = %worker_id, "Worker polling started");

    let idle = Arc::new(AtomicBool::new(false));

    let shutdown = tokio::signal::ctrl_c();

    // ctrl_c gives an !Unpin future
    // tokio::select wants the future it polls to implement Unpin (or are pinned)
    tokio::pin!(shutdown);

    // Single-threaded poller loop with bounded concurrency.
    // The semaphore (max_concurrent permits) gates how many tasks run in parallel.
    // The loop spins freely while permits are available, only sleeping when the
    // previous iteration found no work (idle backoff). Each spawned task holds a
    // permit and releases it on completion, unblocking the next iteration.
    loop {
        if idle.load(Ordering::Relaxed) {
            tokio::time::sleep(poll_interval).await;
            idle.store(false, Ordering::Relaxed);
        }

        tokio::select! {
            _ = &mut shutdown => {
                tracing::info!("Shutting down worker, waiting for in-flight tasks...");
                let timeout = Duration::from_secs(config.worker.shutdown_timeout_sec);
                let _ = tokio::time::timeout(timeout, async {
                    let _all = semaphore.acquire_many(config.worker.max_concurrent as u32).await;
                }).await;
                tracing::info!("Worker shutdown complete");
                return Ok(());
            }
            permit = semaphore.clone().acquire_owned() => {
                let permit = permit?;

                let schemas = match schema_registry.get_active_schemas().await {
                    Ok(s) => s,
                    Err(e) => {
                        tracing::error!("Failed to fetch active schemas: {}", e);
                        drop(permit);
                        tokio::time::sleep(poll_interval).await;
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
}

async fn claim_and_process(
    pool: &PgPool,
    ctx: &PipelineContext,
    schemas: &[String],
    worker_id: &str,
) -> bool {
    for schema_name in schemas {
        let mut tx = match db::scoped::scoped_transaction(pool, schema_name).await {
            Ok(tx) => tx,
            Err(e) => {
                tracing::error!(schema = %schema_name, "Failed to begin scoped transaction: {}", e);
                continue;
            }
        };

        let exec = match db::executions::claim(&mut *tx, worker_id).await {
            Ok(Some(exec)) => exec,
            Ok(None) => continue,
            Err(e) => {
                tracing::error!(schema = %schema_name, "Failed to claim execution: {}", e);
                continue;
            }
        };

        let job = match db::jobs::get(&mut *tx, &exec.job_id).await {
            Ok(Some(job)) => job,
            Ok(None) => {
                tracing::error!(schema = %schema_name, "Associated job for execution {} not found", exec.execution_id);
                continue;
            }
            Err(e) => {
                tracing::error!(schema = %schema_name, "Failed to fetch associated job: {}", e);
                tracing::warn!(schema = %schema_name, "Marking execution as failed: {}", e);
                let _ = db::executions::complete_failed(&mut *tx, &exec.execution_id).await;
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
