use actix_files::Files;
use actix_web::web;
use leptos::prelude::*;
use leptos_actix::{generate_route_list, render_app_to_stream_with_context};
use kronos_dashboard::{app::{App, shell}, config::DashboardConfig};

pub fn configure(
    dashboard_prefix: &str,
    config: DashboardConfig,
    pkg_dir: &str,
) -> impl FnOnce(&mut web::ServiceConfig) + 'static {
    let prefix = dashboard_prefix.to_string();
    let pkg_dir = pkg_dir.to_string();

    move |cfg: &mut web::ServiceConfig| {
        // Serve WASM/JS/CSS assets from the pkg directory
        let pkg_path = if prefix.is_empty() {
            "/pkg".to_string()
        } else {
            format!("{prefix}/pkg")
        };
        tracing::info!("Dashboard: serving pkg files at {pkg_path} from {pkg_dir}");
        cfg.service(Files::new(&pkg_path, &pkg_dir));

        // Generate route list and register SSR routes with prefix
        let routes = generate_route_list(App);
        tracing::info!("Dashboard: generated {} routes", routes.len());
        for listing in routes.iter() {
            let path = listing.path();
            let full_path = if prefix.is_empty() {
                path.to_string()
            } else {
                format!("{prefix}{path}")
            };
            tracing::info!("Dashboard: registering SSR route: {full_path}");

            for method in listing.methods() {
                let config = config.clone();
                let additional_context = move || {
                    provide_context(config.clone());
                };
                cfg.route(
                    &full_path,
                    render_app_to_stream_with_context(
                        additional_context,
                        || shell(App),
                        method,
                    ),
                );
            }
        }
    }
}
