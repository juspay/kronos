//! OIDC `/auth/callback` route handler. Validates `state`, exchanges the
//! authorization `code` for tokens via the IdP's token endpoint (PKCE), decodes
//! claims, stores them in the in-memory [`LoginState`] signal, then navigates
//! back to wherever the user was trying to go.

use base64::Engine;
use leptos::prelude::*;
use leptos_router::hooks::use_query_map;
use serde::Deserialize;

use crate::auth::{pkce, Claims, LoginState};
use crate::config::DashboardConfig;

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    #[serde(default)]
    id_token: Option<String>,
}

/// The dashboard's OIDC callback page. Mounted at the path matching
/// `DashboardConfig::oidc_redirect_url`'s path component (typically
/// `/auth/callback`). Reads `code`/`state` from the query string, pops the
/// PKCE artifacts out of sessionStorage, POSTs to the IdP token endpoint,
/// and on success populates [`LoginState`] and redirects to `return_to`.
#[component]
pub fn CallbackPage() -> impl IntoView {
    let login_state =
        use_context::<RwSignal<LoginState>>().expect("LoginState context not provided");
    let config = use_context::<DashboardConfig>().expect("DashboardConfig context not provided");
    let query = use_query_map();
    let (error, set_error) = signal(Option::<String>::None);

    // Fire the exchange exactly once on mount. We can't use `Action::dispatch`
    // here cleanly because the inputs (query, pkce-from-sessionStorage) are
    // both one-shot reads, so a plain spawn_local in an Effect is the
    // simplest match for the rest of the codebase's idiom.
    Effect::new(move |_| {
        let code = query.read().get("code").map(|s| s.to_string());
        let state = query.read().get("state").map(|s| s.to_string());
        let artifacts = pkce::take();
        let config = config.clone();

        leptos::task::spawn_local(async move {
            let (code, state, artifacts) = match (code, state, artifacts) {
                (Some(c), Some(s), Some(a)) => (c, s, a),
                _ => {
                    set_error.set(Some(
                        "Missing code/state or PKCE artifacts (expired or stale tab?)".into(),
                    ));
                    return;
                }
            };
            if state != artifacts.state {
                set_error.set(Some("State mismatch (possible CSRF)".into()));
                return;
            }
            let issuer = match config.oidc_issuer.as_deref() {
                Some(i) if !i.is_empty() => i,
                _ => {
                    set_error.set(Some("OIDC issuer not configured".into()));
                    return;
                }
            };
            let token_endpoint = match discover_token_endpoint(issuer).await {
                Ok(ep) => ep,
                Err(e) => {
                    set_error.set(Some(format!("Discovery failed: {e}")));
                    return;
                }
            };

            let body = form_urlencoded::Serializer::new(String::new())
                .append_pair("grant_type", "authorization_code")
                .append_pair("code", &code)
                .append_pair(
                    "redirect_uri",
                    config.oidc_redirect_url.as_deref().unwrap_or(""),
                )
                .append_pair("client_id", config.oidc_client_id.as_deref().unwrap_or(""))
                .append_pair("code_verifier", &artifacts.code_verifier)
                .finish();

            let resp = match gloo_net::http::Request::post(&token_endpoint)
                .header("Content-Type", "application/x-www-form-urlencoded")
                .body(body)
            {
                Ok(req) => req,
                Err(e) => {
                    set_error.set(Some(format!("Build request failed: {e}")));
                    return;
                }
            };
            let resp = match resp.send().await {
                Ok(r) => r,
                Err(e) => {
                    set_error.set(Some(format!("Token request failed: {e}")));
                    return;
                }
            };
            let tokens: TokenResponse = match resp.json().await {
                Ok(t) => t,
                Err(e) => {
                    set_error.set(Some(format!("Token response decode failed: {e}")));
                    return;
                }
            };

            let claims = match decode_unverified_claims(&tokens.access_token) {
                Ok(c) => c,
                Err(e) => {
                    set_error.set(Some(format!("Claims decode failed: {e}")));
                    return;
                }
            };
            login_state.set(LoginState {
                access_token: Some(tokens.access_token),
                id_token: tokens.id_token,
                claims: Some(claims),
            });
            if let Some(win) = web_sys::window() {
                let _ = win.location().set_href(&artifacts.return_to);
            }
        });
    });

    view! {
        <div class="p-8">
            <Show
                when=move || error.get().is_some()
                fallback=|| view! { <p>"Completing sign in..."</p> }
            >
                <p class="text-red-600">{move || error.get().unwrap_or_default()}</p>
            </Show>
        </div>
    }
}

async fn discover_token_endpoint(issuer: &str) -> Result<String, String> {
    let url = format!(
        "{}/.well-known/openid-configuration",
        issuer.trim_end_matches('/')
    );
    let body: serde_json::Value = gloo_net::http::Request::get(&url)
        .send()
        .await
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;
    body.get("token_endpoint")
        .and_then(|v| v.as_str())
        .map(String::from)
        .ok_or_else(|| "discovery missing token_endpoint".into())
}

/// Decode JWT claims WITHOUT verifying signature. The access token came
/// from a direct POST to the IdP's token endpoint over TLS using PKCE —
/// the IdP authenticated to us via TLS cert and the JWT was minted by
/// them. The API still validates the JWT against JWKS on every request,
/// so dashboard-side defence-in-depth signature checking is deferred
/// (the API is authoritative).
fn decode_unverified_claims(jwt: &str) -> Result<Claims, String> {
    let parts: Vec<&str> = jwt.split('.').collect();
    if parts.len() != 3 {
        return Err("malformed JWT".into());
    }
    let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(parts[1])
        .map_err(|e| e.to_string())?;
    let raw: serde_json::Value = serde_json::from_slice(&payload).map_err(|e| e.to_string())?;
    Ok(Claims {
        iss: raw["iss"].as_str().unwrap_or_default().to_string(),
        sub: raw["sub"].as_str().unwrap_or_default().to_string(),
        email: raw["email"].as_str().map(String::from),
        name: raw["name"].as_str().map(String::from),
    })
}
