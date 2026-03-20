use kronos_common::{
    cache::{ConfigCache, SecretCache},
    config::AppConfig,
    db, metrics as m,
    tenant::SchemaRegistry,
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
    let schema_registry = SchemaRegistry::new(pool.clone(), 30);

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

                // Try to claim from any active schema
                // TODO 4: A more active workspace (more jobs) can starve other workspaces
                let claimed = {
                    let mut result = None;
                    for schema_name in &schemas {
                        let mut conn = match db::scoped::scoped_connection(&pool, schema_name).await {
                            Ok(c) => c,
                            Err(e) => {
                                tracing::error!(schema = %schema_name, "Failed to get scoped connection: {}", e);
                                continue;
                            }
                        };

                        match db::executions::claim(&mut *conn, &worker_id).await {
                            Ok(Some(exec)) => {
                                result = Some((schema_name.clone(), exec));
                                break;
                            }
                            Ok(None) => continue,
                            Err(e) => {
                                tracing::error!(schema = %schema_name, "Failed to claim execution: {}", e);
                                continue;
                            }
                        }
                    }
                    result
                };

                match claimed {
                    Some((schema, exec)) => {
                        metrics::counter!(m::EXECUTIONS_CLAIMED_TOTAL,
                            "schema" => schema.clone(),
                            "endpoint_type" => exec.endpoint_type.clone(),
                        )
                        .increment(1);

                        let wid = worker_id.clone();
                        metrics::gauge!(m::WORKER_INFLIGHT, "worker_id" => wid.clone())
                            .increment(1.0);

                        let ctx = ctx.clone();
                        // TODO 5: What if the worker dies midway, what happens to the permit? Should we have a monitor for these process_executions
                        tokio::spawn(async move {
                            pipeline::process_execution(
                                &ctx,
                                &schema,
                                &exec.execution_id,
                                &exec.job_id,
                                &exec.endpoint,
                                &exec.endpoint_type,
                                exec.input.as_ref(),
                                exec.attempt_count,
                                exec.max_attempts,
                            ).await;
                            metrics::gauge!(m::WORKER_INFLIGHT, "worker_id" => wid)
                                .decrement(1.0);
                            drop(permit);
                        });
                    }
                    None => {
                        metrics::counter!(m::WORKER_POLL_IDLE_TOTAL,
                            "worker_id" => worker_id.clone(),
                        )
                        .increment(1);
                        drop(permit);
                        tokio::time::sleep(poll_interval).await;
                    }
                }
            }
        }
    }
}
