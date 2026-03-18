use kronos_common::{config::AppConfig, db};
use sqlx::PgPool;
use std::time::Duration;

pub async fn run(pool: PgPool, config: &AppConfig) -> anyhow::Result<()> {
    let interval = Duration::from_secs(config.reclaim_interval_sec);

    tracing::info!(
        "Stuck reclaimer started (interval: {}s, timeout: {}s)",
        config.reclaim_interval_sec,
        config.stuck_execution_timeout_sec
    );

    loop {
        match db::executions::reclaim_stuck(&pool, config.stuck_execution_timeout_sec).await {
            Ok(count) => {
                if count > 0 {
                    tracing::info!("Reclaimed {} stuck executions", count);
                }
            }
            Err(e) => {
                tracing::error!("Stuck reclaimer error: {}", e);
            }
        }
        tokio::time::sleep(interval).await;
    }
}
