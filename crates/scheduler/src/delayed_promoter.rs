use kronos_common::{config::AppConfig, db, tenant::SchemaRegistry};
use sqlx::PgPool;
use std::time::Duration;

pub async fn run(pool: PgPool, config: &AppConfig) -> anyhow::Result<()> {
    let interval = Duration::from_millis(config.promote_interval_ms);
    let schema_registry = SchemaRegistry::new(pool.clone(), 30);

    tracing::info!(
        "Delayed promoter started (interval: {}ms)",
        config.promote_interval_ms
    );

    loop {
        let schemas = schema_registry.get_active_schemas().await.unwrap_or_default();

        for schema_name in &schemas {
            let mut conn = match db::scoped::scoped_connection(&pool, schema_name).await {
                Ok(c) => c,
                Err(e) => {
                    tracing::error!(schema = %schema_name, "Failed to get scoped connection: {}", e);
                    continue;
                }
            };

            match db::executions::promote_pending(&mut *conn).await {
                Ok(count) => {
                    if count > 0 {
                        tracing::debug!(schema = %schema_name, "Promoted {} pending executions to QUEUED", count);
                    }
                }
                Err(e) => {
                    tracing::error!(schema = %schema_name, "Delayed promoter error: {}", e);
                }
            }
        }

        tokio::time::sleep(interval).await;
    }
}
