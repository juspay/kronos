use actix_cors::Cors;
use actix_web::{error::InternalError, web, App, HttpResponse, HttpServer};
use kronos_api::middleware::{AuthMiddleware, AuthMode, AuthState};
use kronos_api::{dashboard, middleware, router};
use kronos_common::config::{AppConfig, ServerMode};
use oidc_rs::{AuthConfig, BasicExchanger, Validator};
use std::sync::Arc;
use tracing_subscriber::EnvFilter;

/// Turn actix's default plaintext body-deserialization errors (e.g.
/// `Json deserialize error: premature end of input at line 1 column 75`)
/// into the structured `{ "error": { code, message } }` shape the rest of
/// the API uses, with a 400 status.
fn json_error_handler(
    err: actix_web::error::JsonPayloadError,
    req: &actix_web::HttpRequest,
) -> actix_web::Error {
    // The `RequestId` middleware stamps every request with an `x-request-id`
    // header; surface it so malformed-body errors are traceable like any other.
    let request_id = req
        .headers()
        .get("x-request-id")
        .and_then(|v| v.to_str().ok())
        .map(|s| serde_json::Value::String(s.to_owned()))
        .unwrap_or(serde_json::Value::Null);
    let message = format!("Malformed JSON request body: {err}");
    let response = HttpResponse::BadRequest().json(serde_json::json!({
        "error": {
            "code": "INVALID_REQUEST",
            "message": message,
            "request_id": request_id,
        }
    }));
    InternalError::from_response(err, response).into()
}

/// Build the runtime auth state from the parsed `AuthConfig`. Fails fast on
/// initial IdP unreachability — preferring a crash-loop to silently serving
/// unauthenticated traffic. Operators running a rolling deploy during an
/// IdP outage will see new pods refuse to start; the old pods keep serving.
async fn build_auth_state(cfg: &AuthConfig) -> anyhow::Result<Arc<AuthState>> {
    let mode = match cfg {
        AuthConfig::Disabled => {
            tracing::warn!(
                "Auth disabled (TE_AUTH_MODE=disabled). Do not use in production."
            );
            AuthMode::Disabled
        }
        AuthConfig::Enabled(c) => {
            let validator =
                Validator::new(c.issuer.clone(), c.audiences.clone(), c.jwks_refresh)
                    .await
                    .map_err(|e| anyhow::anyhow!("validator init: {e}"))?;
            let exchanger = BasicExchanger::new(
                c.issuer.clone(),
                c.basic_audience.clone(),
                c.basic_scope.clone(),
                c.basic_cache_ttl,
            )
            .await
            .map_err(|e| anyhow::anyhow!("exchanger init: {e}"))?;
            AuthMode::Enabled {
                validator,
                exchanger,
            }
        }
    };
    Ok(Arc::new(AuthState { mode }))
}

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

    let auth_state = build_auth_state(&config.auth).await?;

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
        let (auth_disabled, oidc_issuer, oidc_client_id, oidc_redirect_url, oidc_audience) =
            match &config.auth {
                oidc_rs::AuthConfig::Disabled => (true, None, None, None, None),
                oidc_rs::AuthConfig::Enabled(c) => {
                    let issuer = Some(c.issuer.clone());
                    // Three extra envs let the operator configure the dashboard's
                    // public OIDC client without baking it into AuthConfig.
                    let client_id = std::env::var("TE_OIDC_DASHBOARD_CLIENT_ID").ok();
                    let redirect_url = std::env::var("TE_OIDC_DASHBOARD_REDIRECT_URL").ok();
                    // Audience for the `/authorize` request. For Auth0 this is
                    // REQUIRED to get an API-scoped access_token (otherwise the
                    // token's `aud` is `https://<tenant>.auth0.com/userinfo` and
                    // the API rejects it). For Keycloak / Okta it's typically
                    // unused / harmless. NEVER fall back to `client_id` here —
                    // they are independent concepts in every IdP we support.
                    let audience = std::env::var("TE_OIDC_DASHBOARD_AUDIENCE").ok();
                    (false, issuer, client_id, redirect_url, audience)
                }
            };

        // Surface the misconfiguration loud: auth is enabled, the dashboard
        // is being served, but the two envs the PKCE flow needs aren't set.
        // Without these the dashboard will redirect to the IdP with an empty
        // `client_id` / `redirect_uri` and fail every login at runtime —
        // tracing this back from a user bug report is painful, so we
        // pre-warn at startup.
        if !matches!(mode, ServerMode::Api)
            && matches!(config.auth, oidc_rs::AuthConfig::Enabled(_))
            && (oidc_client_id.is_none() || oidc_redirect_url.is_none())
        {
            tracing::warn!(
                "Auth is enabled and dashboard is being served, but \
                 TE_OIDC_DASHBOARD_CLIENT_ID and/or TE_OIDC_DASHBOARD_REDIRECT_URL \
                 are not set — the dashboard login flow will fail at runtime."
            );
        }

        Some(kronos_dashboard::config::DashboardConfig {
            api_base_url: String::new(), // same-origin; server functions handle routing
            api_prefix: path_prefix.clone(),
            dashboard_prefix: dashboard_prefix.clone(),
            auth_disabled,
            oidc_issuer,
            oidc_client_id,
            oidc_redirect_url,
            oidc_audience,
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

        let auth_mw = AuthMiddleware {
            state: auth_state.clone(),
        };

        let mut app = App::new()
            .app_data(web::Data::new(app_state.clone()))
            .app_data(web::Data::from(auth_state.clone()))
            .app_data(web::JsonConfig::default().error_handler(json_error_handler))
            .wrap(cors)
            .wrap(actix_web::middleware::Logger::default())
            .wrap(middleware::RequestId);

        // Register API routes (specific paths first)
        if mode == ServerMode::Api || mode == ServerMode::Both {
            app = app.configure(router::configure(
                &path_prefix,
                &mode,
                &dashboard_prefix,
                auth_mw,
            ));
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
