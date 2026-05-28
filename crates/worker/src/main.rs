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

    // Provision kronos's own internal jobs (today: the dogfooded reaper) for
    // every active workspace, then keep them in sync as new workspaces appear.
    // The reaper sweep itself runs as a CRON-triggered execution claimed by the
    // poller — see `worker::bootstrap`. Aborted when the poller returns on
    // shutdown; every step is idempotent so a mid-pass abort recovers next pass.
    let bootstrap = tokio::spawn(kronos_worker::bootstrap::run(pool.clone(), config.clone()));

    kronos_worker::poller::run(pool, config).await?;

    bootstrap.abort();

    Ok(())
}
