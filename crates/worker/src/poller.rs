use kronos_common::{
    cache::{ConfigCache, SecretCache},
    config::AppConfig,
    db,
};
use reqwest::Client;
use sqlx::PgPool;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Semaphore;
use uuid::Uuid;

use crate::pipeline::{self, PipelineContext};

pub async fn run(pool: PgPool, config: AppConfig) -> anyhow::Result<()> {
    let worker_id = format!("worker_{}", Uuid::new_v4().simple());
    let semaphore = Arc::new(Semaphore::new(config.worker_max_concurrent));
    let poll_interval = Duration::from_millis(config.worker_poll_interval_ms);

    let ctx = Arc::new(PipelineContext {
        pool: pool.clone(),
        http_client: Client::new(),
        config_cache: ConfigCache::new(config.config_cache_ttl_sec),
        secret_cache: SecretCache::new(config.secret_cache_ttl_sec),
        encryption_key: config.encryption_key.clone(),
    });

    tracing::info!(worker_id = %worker_id, "Worker polling started");

    let shutdown = tokio::signal::ctrl_c();
    tokio::pin!(shutdown);

    loop {
        tokio::select! {
            _ = &mut shutdown => {
                tracing::info!("Shutting down worker, waiting for in-flight tasks...");
                // Wait for all permits to be returned (all tasks done)
                let timeout = Duration::from_secs(config.worker_shutdown_timeout_sec);
                let _ = tokio::time::timeout(timeout, async {
                    // Acquire all permits = all tasks done
                    let _all = semaphore.acquire_many(config.worker_max_concurrent as u32).await;
                }).await;
                tracing::info!("Worker shutdown complete");
                return Ok(());
            }
            permit = semaphore.clone().acquire_owned() => {
                let permit = permit?;

                match db::executions::claim(&pool, &worker_id).await {
                    Ok(Some(exec)) => {
                        let ctx = ctx.clone();
                        tokio::spawn(async move {
                            pipeline::process_execution(
                                &ctx,
                                &exec.execution_id,
                                &exec.job_id,
                                &exec.endpoint,
                                &exec.endpoint_type,
                                exec.input.as_ref(),
                                exec.attempt_count,
                                exec.max_attempts,
                            ).await;
                            drop(permit);
                        });
                    }
                    Ok(None) => {
                        drop(permit);
                        tokio::time::sleep(poll_interval).await;
                    }
                    Err(e) => {
                        tracing::error!("Failed to claim execution: {}", e);
                        drop(permit);
                        tokio::time::sleep(poll_interval).await;
                    }
                }
            }
        }
    }
}
