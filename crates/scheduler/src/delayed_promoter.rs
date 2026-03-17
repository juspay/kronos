use kronos_common::{config::AppConfig, db};
use sqlx::PgPool;
use std::time::Duration;

pub async fn run(pool: PgPool, config: &AppConfig) -> anyhow::Result<()> {
    let interval = Duration::from_millis(config.promote_interval_ms);

    tracing::info!("Delayed promoter started (interval: {}ms)", config.promote_interval_ms);

    loop {
        match db::executions::promote_pending(&pool).await {
            Ok(count) => {
                if count > 0 {
                    tracing::debug!("Promoted {} pending executions to QUEUED", count);
                }
            }
            Err(e) => {
                tracing::error!("Delayed promoter error: {}", e);
            }
        }
        tokio::time::sleep(interval).await;
    }
}
