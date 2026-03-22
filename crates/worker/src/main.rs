use kronos_common::config::AppConfig;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();

    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("kronos=debug".parse()?))
        .json()
        .init();

    let config = AppConfig::from_env()?;
    let pool = sqlx::PgPool::connect(&config.database_url).await?;

    kronos_common::metrics::install_recorder_with_listener(config.metrics_port);

    tracing::info!("Worker starting (metrics on port {})", config.metrics_port);

    // Run stuck reclaimer as a background safety net
    tokio::spawn(kronos_worker::stuck_reclaimer::run(
        pool.clone(),
        config.reclaim_interval_sec,
        config.stuck_execution_timeout_sec,
    ));

    kronos_worker::poller::run(pool, config).await?;

    Ok(())
}
