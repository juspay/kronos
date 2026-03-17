use kronos_common::config::AppConfig;
use tracing_subscriber::EnvFilter;

mod backoff;
mod dispatcher;
mod pipeline;
mod poller;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();

    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("kronos=debug".parse()?))
        .json()
        .init();

    let config = AppConfig::from_env()?;
    let pool = sqlx::PgPool::connect(&config.database_url).await?;

    tracing::info!("Worker starting");
    poller::run(pool, config).await?;

    Ok(())
}
