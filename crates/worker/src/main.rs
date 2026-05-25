use kronos_common::config::AppConfig;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();

    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("kronos=debug".parse()?))
        .json()
        .init();

    let config = AppConfig::from_env().await?;
    let pool = sqlx::PgPool::connect(&config.db.url).await?;

    kronos_common::metrics::install_recorder_with_listener(config.metrics.port);

    tracing::info!("Worker starting (metrics on port {})", config.metrics.port);

    // Reap expired CRON jobs in the background (retire + unschedule from pg_cron).
    // Aborted when the poller returns on shutdown; its work is idempotent so a
    // mid-sweep abort is recovered on the next process start.
    let reaper = tokio::spawn(kronos_worker::reaper::run(pool.clone(), config.clone()));

    kronos_worker::poller::run(pool, config).await?;

    reaper.abort();

    Ok(())
}
