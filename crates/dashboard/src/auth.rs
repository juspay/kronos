//! Dashboard auth state. Submodules `pkce` and `callback` land in Task 14.

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
