use kronos_common::{
    cache::{ConfigCache, SecretCache},
    config::AppConfig,
    db,
    kms::{self, KmsProvider},
    metrics as m,
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
    let semaphore = Arc::new(Semaphore::new(config.worker_max_concurrent));
    let poll_interval = Duration::from_millis(config.worker_poll_interval_ms);
    let schema_registry = SchemaRegistry::new(pool.clone(), 30);

    let kms_provider: Arc<dyn KmsProvider> = match config.kms_provider.as_str() {
        "aws" => Arc::new(
            kms::aws::AwsKmsProvider::new(
                config.kms_aws_region.clone(),
                config.kms_aws_endpoint_url.clone(),
            )
            .await
            .expect("Failed to initialize AWS KMS provider"),
        ),
        other => anyhow::bail!("Unsupported KMS provider: {}", other),
    };

    let secret_cache = SecretCache::new(config.secret_cache_ttl_sec);

    let ctx = Arc::new(PipelineContext {
        pool: pool.clone(),
        http_client: Client::new(),
        config_cache: ConfigCache::new(config.config_cache_ttl_sec),
        secret_cache,
        kms_provider,
    });

    // Preload secrets from KMS at startup
    preload_secrets(&pool, &schema_registry, &ctx).await;

    tracing::info!(worker_id = %worker_id, "Worker polling started");

    let idle = Arc::new(AtomicBool::new(false));
    let shutdown = tokio::signal::ctrl_c();
    tokio::pin!(shutdown);

    loop {
        if idle.load(Ordering::Relaxed) {
            tokio::time::sleep(poll_interval).await;
            idle.store(false, Ordering::Relaxed);
        }

        tokio::select! {
            _ = &mut shutdown => {
                tracing::info!("Shutting down worker, waiting for in-flight tasks...");
                let timeout = Duration::from_secs(config.worker_shutdown_timeout_sec);
                let _ = tokio::time::timeout(timeout, async {
                    let _all = semaphore.acquire_many(config.worker_max_concurrent as u32).await;
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

        metrics::counter!(m::EXECUTIONS_CLAIMED_TOTAL,
            "schema" => schema_name.clone(),
            "endpoint_type" => exec.endpoint_type.clone(),
        )
        .increment(1);

        metrics::gauge!(m::WORKER_INFLIGHT, "worker_id" => worker_id.to_string())
            .increment(1.0);

        pipeline::process_execution(
            ctx,
            &mut *tx,
            schema_name,
            &exec.execution_id,
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

        metrics::gauge!(m::WORKER_INFLIGHT, "worker_id" => worker_id.to_string())
            .decrement(1.0);

        return true;
    }

    false
}

async fn preload_secrets(pool: &PgPool, schema_registry: &SchemaRegistry, ctx: &PipelineContext) {
    let schemas = match schema_registry.get_active_schemas().await {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!("Failed to fetch schemas for secret preload: {}", e);
            return;
        }
    };

    let mut total = 0u64;
    let mut failed = 0u64;

    for schema_name in &schemas {
        let mut conn = match db::scoped::scoped_connection(pool, schema_name).await {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!(schema = %schema_name, "Failed to get connection for secret preload: {}", e);
                continue;
            }
        };

        let secrets = match db::secrets::list_all(&mut *conn).await {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!(schema = %schema_name, "Failed to list secrets for preload: {}", e);
                continue;
            }
        };

        for secret in secrets {
            total += 1;
            match ctx.kms_provider.get_secret(&secret.reference).await {
                Ok(value) => {
                    ctx.secret_cache.set(secret.name, value);
                }
                Err(e) => {
                    failed += 1;
                    tracing::warn!(
                        schema = %schema_name,
                        secret = %secret.name,
                        "Failed to preload secret from KMS: {}. Will be fetched on demand.",
                        e
                    );
                }
            }
        }
    }

    tracing::info!(
        total = total,
        failed = failed,
        "Secret preload complete"
    );
}
