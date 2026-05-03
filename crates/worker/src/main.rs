use kronos_common::config::AppConfig;
use kronos_embedded_worker::Worker;
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

    tracing::info!("Worker starting (metrics on port {})", config.metrics.port);

    Worker::builder(pool)
        .from_app_config(&config)
        .install_metrics_recorder(true)
        .build()
        .await?
        .run_until_ctrl_c()
        .await
}
