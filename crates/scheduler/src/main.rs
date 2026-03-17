use kronos_common::config::AppConfig;
use tracing_subscriber::EnvFilter;

mod cron_materializer;
mod delayed_promoter;
mod stuck_reclaimer;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();

    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("kronos=debug".parse()?))
        .json()
        .init();

    let config = AppConfig::from_env()?;
    let pool = sqlx::PgPool::connect(&config.database_url).await?;

    tracing::info!("Scheduler starting");

    tokio::select! {
        r = cron_materializer::run(pool.clone(), &config) => r?,
        r = delayed_promoter::run(pool.clone(), &config) => r?,
        r = stuck_reclaimer::run(pool.clone(), &config) => r?,
        _ = tokio::signal::ctrl_c() => {
            tracing::info!("Scheduler shutting down");
        }
    }

    Ok(())
}
