use kronos_common::{config::AppConfig, tenant::SchemaRegistry};
use tokio_util::sync::CancellationToken;
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

    // Standalone: use Kronos's own public.workspaces table for schema discovery.
    let schema_provider = SchemaRegistry::new(pool.clone(), 30);

    // Cancel on Ctrl-C
    let cancel = CancellationToken::new();
    let cancel_clone = cancel.clone();
    tokio::spawn(async move {
        let _ = tokio::signal::ctrl_c().await;
        tracing::info!("Ctrl-C received, cancelling worker...");
        cancel_clone.cancel();
    });

    kronos_worker::poller::run(pool, config, schema_provider, cancel).await?;

    Ok(())
}
