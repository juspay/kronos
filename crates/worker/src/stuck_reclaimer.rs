use kronos_common::{db, metrics as m, tenant::SchemaRegistry};
use sqlx::PgPool;
use std::time::Duration;

pub async fn run(pool: PgPool, reclaim_interval_sec: u64, stuck_timeout_sec: i64) -> anyhow::Result<()> {
    let interval = Duration::from_secs(reclaim_interval_sec);
    let schema_registry = SchemaRegistry::new(pool.clone(), 30);

    tracing::info!(
        "Stuck reclaimer started (interval: {}s, timeout: {}s)",
        reclaim_interval_sec,
        stuck_timeout_sec
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

            match db::executions::reclaim_stuck(&mut *conn, stuck_timeout_sec).await {
                Ok(count) => {
                    if count > 0 {
                        tracing::info!(schema = %schema_name, "Reclaimed {} stuck executions", count);
                        metrics::counter!(m::EXECUTIONS_RECLAIMED_TOTAL,
                            "schema" => schema_name.clone(),
                        )
                        .increment(count);
                    }
                }
                Err(e) => {
                    tracing::error!(schema = %schema_name, "Stuck reclaimer error: {}", e);
                }
            }
        }

        tokio::time::sleep(interval).await;
    }
}
