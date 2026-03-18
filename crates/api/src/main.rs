use actix_web::{web, App, HttpServer};
use kronos_common::config::AppConfig;
use sqlx::migrate::Migrator;
use tracing_subscriber::EnvFilter;

mod extractors;
mod handlers;
mod middleware;
mod router;

#[actix_web::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();

    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("kronos=debug".parse()?))
        .json()
        .init();

    let config = AppConfig::from_env()?;
    let pool = sqlx::PgPool::connect(&config.database_url).await?;

    // CockroachDB doesn't support pg_advisory_lock, so disable locking.
    // CARGO_MANIFEST_DIR is resolved at compile time so the path works regardless of cwd.
    let mut migrator = Migrator::new(
        std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../migrations"))
    ).await?;
    migrator.set_locking(false).run(&pool).await?;

    let listen_addr = config.listen_addr.clone();
    let app_state = router::AppState {
        pool: pool.clone(),
        config: config.clone(),
    };

    tracing::info!("API server listening on {}", listen_addr);

    HttpServer::new(move || {
        App::new()
            .app_data(web::Data::new(app_state.clone()))
            .wrap(crate::middleware::RequestId)
            .configure(router::configure)
    })
    .bind(&listen_addr)?
    .run()
    .await?;

    Ok(())
}
