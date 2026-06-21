//! Dashboard auth state. PKCE generation and the `/auth/callback` route
//! handler live in submodules and are hydrate-only (WASM-side APIs).

#[cfg(feature = "hydrate")]
mod callback;
#[cfg(feature = "hydrate")]
mod pkce;

#[cfg(feature = "hydrate")]
pub use callback::CallbackPage;
#[cfg(feature = "hydrate")]
pub use pkce::{generate_pkce, PkceArtifacts};
#[cfg(feature = "hydrate")]
pub use redirect_helpers::{logout, redirect_to_idp};

use leptos::prelude::*;
use serde::{Deserialize, Serialize};

/// In-memory token state for the dashboard. Holds the access/id tokens for
/// the duration of the page session — never persisted to web storage.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct LoginState {
    pub access_token: Option<String>,
    pub id_token: Option<String>,
    pub claims: Option<Claims>,
}

/// Minimal subset of OIDC claims the dashboard needs to render "logged in as".
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Claims {
    pub iss: String,
    pub sub: String,
    pub email: Option<String>,
    pub name: Option<String>,
}

/// Install a fresh, empty `LoginState` signal in the Leptos context.
/// Call once at the root of the app.
pub fn provide_login_state() {
    provide_context(RwSignal::new(LoginState::default()));
}

#[cfg(feature = "hydrate")]
mod redirect_helpers {
    use crate::auth::{pkce, LoginState};
    use crate::config::DashboardConfig;
    use form_urlencoded::byte_serialize;
    use leptos::prelude::*;

    /// Build the IdP `/authorize` URL with PKCE artifacts persisted in
    /// sessionStorage, then navigate to it. Called when the dashboard
    /// detects no access token on a protected route.
    pub fn redirect_to_idp(config: &DashboardConfig, return_to: &str) {
        let issuer = config.oidc_issuer.as_deref().unwrap_or("");
        let client_id = config.oidc_client_id.as_deref().unwrap_or("");
        let redirect_url = config.oidc_redirect_url.as_deref().unwrap_or("");

        let artifacts = pkce::generate_pkce(return_to);
        let challenge = pkce::code_challenge(&artifacts.code_verifier);
        pkce::persist(&artifacts);

        let authorize = format!("{}/authorize", issuer.trim_end_matches('/'));
        let mut url = format!(
            "{authorize}?response_type=code\
             &client_id={cid}\
             &redirect_uri={ru}\
             &scope=openid%20email%20profile\
             &state={state}\
             &nonce={nonce}\
             &code_challenge={ch}\
             &code_challenge_method=S256",
            authorize = authorize,
            cid = url_encode(client_id),
            ru = url_encode(redirect_url),
            state = url_encode(&artifacts.state),
            nonce = url_encode(&artifacts.nonce),
            ch = url_encode(&challenge),
        );
        // Auth0 (and any IdP that issues API-scoped access tokens) requires
        // the `audience` parameter at /authorize time. Without it Auth0 issues
        // an opaque `/userinfo`-scoped access_token whose `aud` claim does NOT
        // match TE_OIDC_AUDIENCES; the API then rejects every request.
        if let Some(audience) = config.oidc_audience.as_deref() {
            if !audience.is_empty() {
                url.push_str(&format!("&audience={}", url_encode(audience)));
            }
        }
        if let Some(w) = web_sys::window() {
            let _ = w.location().set_href(&url);
        }
    }

    /// Clear in-memory tokens, then (best-effort) redirect to the IdP's
    /// `end_session_endpoint` if discovery advertises one. Otherwise navigate
    /// to `/`.
    pub fn logout(config: &DashboardConfig, login_state: RwSignal<LoginState>) {
        let id_token = login_state.with(|s| s.id_token.clone());
        login_state.set(LoginState::default());
        let issuer = match config.oidc_issuer.as_deref() {
            Some(i) => i.to_string(),
            None => {
                if let Some(w) = web_sys::window() {
                    let _ = w.location().set_href("/");
                }
                return;
            }
        };
        leptos::task::spawn_local(async move {
            let discovery_url = format!(
                "{}/.well-known/openid-configuration",
                issuer.trim_end_matches('/')
            );
            if let Ok(resp) = gloo_net::http::Request::get(&discovery_url).send().await {
                if let Ok(body) = resp.json::<serde_json::Value>().await {
                    if let Some(end_session) =
                        body.get("end_session_endpoint").and_then(|v| v.as_str())
                    {
                        let mut url = end_session.to_string();
                        if let Some(t) = &id_token {
                            url.push_str(&format!("?id_token_hint={}", url_encode(t)));
                        }
                        if let Some(w) = web_sys::window() {
                            let _ = w.location().set_href(&url);
                            return;
                        }
                    }
                }
            }
            if let Some(w) = web_sys::window() {
                let _ = w.location().set_href("/");
            }
        });
    }

    fn url_encode(s: &str) -> String {
        byte_serialize(s.as_bytes()).collect()
    }
}
