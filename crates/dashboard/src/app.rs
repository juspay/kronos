use leptos::prelude::*;
use leptos_meta::*;
use leptos_router::{
    components::{Route, Router, Routes},
    path,
};

use crate::components::sidebar::Sidebar;
use crate::config::DashboardConfig;
use crate::pages::{
    org_detail::OrgDetailPage,
    organizations::OrganizationsPage,
    workspace_detail::WorkspaceDetailPage,
};

pub fn dashboard_prefix() -> String {
    use_context::<DashboardConfig>()
        .map(|c| c.dashboard_prefix.clone())
        .unwrap_or_default()
}

pub fn prefixed(path: &str) -> String {
    format!("{}{path}", dashboard_prefix())
}

fn pkg_base() -> String {
    use_context::<DashboardConfig>()
        .map(|c| {
            if c.dashboard_prefix.is_empty() {
                "/pkg".to_string()
            } else {
                format!("{}/pkg", c.dashboard_prefix)
            }
        })
        .unwrap_or_else(|| "/pkg".to_string())
}

/// The SSR shell — wraps the App in a full HTML document.
/// Only used during server-side rendering, NOT during hydration.
#[cfg(feature = "ssr")]
pub fn shell(app: impl IntoView) -> impl IntoView {
    let config_script = use_context::<DashboardConfig>()
        .map(|c| {
            format!(
                r#"window.__KRONOS_CONFIG__={{apiBaseUrl:"{}",apiPrefix:"{}",dashboardPrefix:"{}",authDisabled:"{}",oidcIssuer:"{}",oidcClientId:"{}",oidcRedirectUrl:"{}",oidcAudience:"{}"}};"#,
                c.api_base_url,
                c.api_prefix,
                c.dashboard_prefix,
                if c.auth_disabled { "true" } else { "false" },
                c.oidc_issuer.as_deref().unwrap_or(""),
                c.oidc_client_id.as_deref().unwrap_or(""),
                c.oidc_redirect_url.as_deref().unwrap_or(""),
                c.oidc_audience.as_deref().unwrap_or(""),
            )
        })
        .unwrap_or_default();

    let pkg = pkg_base();
    let wasm_script = format!(
        r#"import init, {{ hydrate }} from '{pkg}/kronos_dashboard.js'; async function main() {{ await init('{pkg}/kronos_dashboard_bg.wasm'); hydrate(); }} main();"#
    );

    view! {
        <!DOCTYPE html>
        <html lang="en">
            <head>
                <meta charset="utf-8" />
                <meta name="viewport" content="width=device-width, initial-scale=1" />
            </head>
            <body>
                <script>{config_script}</script>
                {app}
                <script type="module">{wasm_script}</script>
            </body>
        </html>
    }
}

/// The App component — renders only body content.
/// Shared between SSR and hydration.
#[component]
pub fn App() -> impl IntoView {
    provide_meta_context();
    crate::auth::provide_login_state();

    // During hydration, read config from the injected window.__KRONOS_CONFIG__
    #[cfg(feature = "hydrate")]
    {
        if use_context::<DashboardConfig>().is_none() {
            use wasm_bindgen::JsValue;
            let window = web_sys::window().expect("no global window");
            let config = js_sys::Reflect::get(&window, &JsValue::from_str("__KRONOS_CONFIG__"))
                .unwrap_or(JsValue::UNDEFINED);
            let get = |key: &str| -> String {
                if config.is_undefined() || config.is_null() {
                    return String::new();
                }
                js_sys::Reflect::get(&config, &JsValue::from_str(key))
                    .ok()
                    .and_then(|v| v.as_string())
                    .unwrap_or_default()
            };
            let get_opt = |key: &str| -> Option<String> {
                let v = get(key);
                if v.is_empty() { None } else { Some(v) }
            };
            let get_bool = |key: &str| -> bool {
                matches!(get(key).as_str(), "1" | "true" | "yes")
            };
            provide_context(DashboardConfig {
                api_base_url: get("apiBaseUrl"),
                api_prefix: get("apiPrefix"),
                dashboard_prefix: get("dashboardPrefix"),
                auth_disabled: get_bool("authDisabled"),
                oidc_issuer: get_opt("oidcIssuer"),
                oidc_client_id: get_opt("oidcClientId"),
                oidc_redirect_url: get_opt("oidcRedirectUrl"),
                oidc_audience: get_opt("oidcAudience"),
            });
        }
    }

    // When auth is enabled and the user has no access token in `LoginState`,
    // bounce to the IdP. Skip when we're already on the callback page (where
    // the token exchange runs). Hydrate-only — SSR has no concept of an
    // interactive login redirect.
    #[cfg(feature = "hydrate")]
    {
        let config =
            use_context::<DashboardConfig>().expect("DashboardConfig context not provided");
        if !config.auth_disabled {
            let login_state = use_context::<RwSignal<crate::auth::LoginState>>()
                .expect("LoginState context not provided");
            let cfg_for_effect = config.clone();
            Effect::new(move |_| {
                if login_state.with(|s| s.access_token.is_none()) {
                    let path = web_sys::window()
                        .and_then(|w| w.location().pathname().ok())
                        .unwrap_or_default();
                    if !path.contains("/auth/callback") {
                        crate::auth::redirect_to_idp(&cfg_for_effect, &path);
                    }
                }
            });
        }
    }

    let base = dashboard_prefix();
    let css_href = format!("{}/tailwind-output.css", pkg_base());

    view! {
        <Stylesheet href=css_href />
        <Title text="Kronos Dashboard" />
        <Router base=base>
            <div class="flex min-h-screen">
                <Sidebar />
                <main class="flex-1 p-8">
                    <Routes fallback=|| "Page not found.">
                        <Route path=path!("/") view=OrganizationsPage />
                        <Route path=path!("/orgs/:org_id") view=OrgDetailPage />
                        <Route path=path!("/orgs/:org_id/workspaces/:workspace_id") view=WorkspaceDetailPage />
                        <Route path=path!("/auth/callback") view=AuthCallbackRoute />
                    </Routes>
                </main>
            </div>
        </Router>
    }
}

/// Thin wrapper component for the `/auth/callback` route. Under SSR the
/// real `CallbackPage` is unavailable (it depends on web-sys + gloo), so we
/// render a placeholder; hydration replaces it with the real component.
#[component]
fn AuthCallbackRoute() -> impl IntoView {
    #[cfg(feature = "hydrate")]
    {
        view! { <crate::auth::CallbackPage /> }.into_any()
    }
    #[cfg(not(feature = "hydrate"))]
    {
        view! { <div class="p-8">"Completing sign in..."</div> }.into_any()
    }
}
