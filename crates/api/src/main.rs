use actix_cors::Cors;
use actix_web::{web, App, HttpServer};
use kronos_common::config::{AppConfig, ServerMode};
use tracing_subscriber::EnvFilter;

mod dashboard;
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

    let config = AppConfig::from_env().await?;
    let pool = sqlx::PgPool::connect(&config.db.url).await?;

    let metrics_handle = kronos_common::metrics::install_recorder();

    let listen_addr = config.server.listen_addr.clone();
    let path_prefix = config.server.path_prefix.clone();
    let mode = config.server.mode.clone();
    let dashboard_prefix = config.server.dashboard_prefix.clone();
    let dashboard_dist_dir = config.server.dashboard_dist_dir.clone();

    let app_state = router::AppState {
        pool: pool.clone(),
        config: config.clone(),
        metrics_handle,
    };

    tracing::info!("Server mode: {:?}", mode);
    tracing::info!("API server listening on {}", listen_addr);
    if !path_prefix.is_empty() {
        tracing::info!("API path prefix: {}", path_prefix);
    }

    // Build dashboard config if needed
    let dashboard_config = if mode != ServerMode::Api {
        tracing::info!(
            "Dashboard dist dir: {}, prefix: {:?}",
            dashboard_dist_dir,
            dashboard_prefix
        );
        Some(kronos_dashboard::config::DashboardConfig {
            api_base_url: String::new(), // same-origin; server functions handle routing
            api_prefix: path_prefix.clone(),
            dashboard_prefix: dashboard_prefix.clone(),
            api_key: config.server.api_key.clone(),
        })
    } else {
        None
    };

    HttpServer::new(move || {
        let cors = Cors::default()
            .allow_any_origin()
            .allow_any_method()
            .allow_any_header()
            .max_age(3600);

        let mut app = App::new()
            .app_data(web::Data::new(app_state.clone()))
            .wrap(cors)
            .wrap(crate::middleware::RequestId);

        // Register API routes (specific paths first)
        if mode == ServerMode::Api || mode == ServerMode::Both {
            app = app.configure(router::configure(&path_prefix, &mode, &dashboard_prefix));
        }

        // Register dashboard routes (catch-all last)
        if let Some(ref dc) = dashboard_config {
            app = app.configure(dashboard::configure(
                &dashboard_prefix,
                dc.clone(),
                &dashboard_dist_dir,
            ));
        }

        app
    })
    .bind(&listen_addr)?
    .run()
    .await?;

    Ok(())
}
