//! Kronos-specific bridge between `TE_*` environment variables and the
//! framework-agnostic `oidc_rs::AuthConfig` builder.

use crate::env::get_from_env_or_default;
use oidc_rs::AuthConfig;
use std::time::Duration;

/// Parse Kronos's `TE_*` env vars into an `oidc_rs::AuthConfig`.
///
/// `TE_AUTH_MODE=disabled` → [`AuthConfig::Disabled`].
/// `TE_AUTH_MODE=enabled` (default) → requires `TE_OIDC_ISSUER` and
/// `TE_OIDC_AUDIENCES` and uses the optional `TE_OIDC_BASIC_*` and
/// `TE_OIDC_JWKS_REFRESH_SEC` vars where set.
pub fn read_auth_config_from_env() -> Result<AuthConfig, String> {
    let mode = get_from_env_or_default("TE_AUTH_MODE", "enabled".to_string())
        .to_lowercase();
    match mode.as_str() {
        "disabled" => Ok(AuthConfig::disabled()),
        "enabled" => build_enabled(),
        other => Err(format!(
            "TE_AUTH_MODE must be 'enabled' or 'disabled', got '{other}'"
        )),
    }
}

fn build_enabled() -> Result<AuthConfig, String> {
    let issuer = std::env::var("TE_OIDC_ISSUER")
        .map_err(|_| "TE_OIDC_ISSUER is required when TE_AUTH_MODE=enabled".to_string())?;
    let audiences_raw = std::env::var("TE_OIDC_AUDIENCES").map_err(|_| {
        "TE_OIDC_AUDIENCES is required when TE_AUTH_MODE=enabled \
         (comma-separated list of accepted `aud` values)"
            .to_string()
    })?;
    let audiences: Vec<String> = audiences_raw
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    if audiences.is_empty() {
        return Err("TE_OIDC_AUDIENCES must contain at least one value".into());
    }
    let mut builder = AuthConfig::builder().issuer(issuer).audiences(audiences);
    if let Ok(a) = std::env::var("TE_OIDC_BASIC_AUDIENCE") {
        if !a.is_empty() {
            builder = builder.basic_audience(a);
        }
    }
    if let Ok(s) = std::env::var("TE_OIDC_BASIC_SCOPE") {
        if !s.is_empty() {
            builder = builder.basic_scope(s);
        }
    }
    let basic_cache_ttl = Duration::from_secs(get_from_env_or_default(
        "TE_OIDC_BASIC_CACHE_TTL_SEC",
        3600u64,
    ));
    let jwks_refresh = Duration::from_secs(get_from_env_or_default(
        "TE_OIDC_JWKS_REFRESH_SEC",
        300u64,
    ));
    builder
        .basic_cache_ttl(basic_cache_ttl)
        .jwks_refresh(jwks_refresh)
        .build()
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with_env<F: FnOnce()>(vars: &[(&str, Option<&str>)], f: F) {
        let saved: Vec<(String, Option<String>)> = vars
            .iter()
            .map(|(k, _)| (k.to_string(), std::env::var(k).ok()))
            .collect();
        for (k, v) in vars {
            match v {
                Some(val) => std::env::set_var(k, val),
                None => std::env::remove_var(k),
            }
        }
        f();
        for (k, v) in saved {
            match v {
                Some(val) => std::env::set_var(&k, val),
                None => std::env::remove_var(&k),
            }
        }
    }

    #[test]
    fn disabled_round_trip() {
        with_env(
            &[
                ("TE_AUTH_MODE", Some("disabled")),
                ("TE_OIDC_ISSUER", None),
                ("TE_OIDC_AUDIENCES", None),
            ],
            || {
                assert!(matches!(read_auth_config_from_env(), Ok(AuthConfig::Disabled)));
            },
        );
    }

    #[test]
    fn enabled_requires_issuer_and_audiences() {
        with_env(
            &[
                ("TE_AUTH_MODE", Some("enabled")),
                ("TE_OIDC_ISSUER", None),
                ("TE_OIDC_AUDIENCES", None),
            ],
            || {
                let err = read_auth_config_from_env().unwrap_err();
                assert!(err.contains("TE_OIDC_ISSUER"));
            },
        );
    }

    #[test]
    fn enabled_full_config() {
        with_env(
            &[
                ("TE_AUTH_MODE", Some("enabled")),
                ("TE_OIDC_ISSUER", Some("https://idp.example.com")),
                ("TE_OIDC_AUDIENCES", Some("api,dashboard")),
                ("TE_OIDC_BASIC_AUDIENCE", Some("api")),
                ("TE_OIDC_BASIC_SCOPE", Some("jobs.read")),
                ("TE_OIDC_BASIC_CACHE_TTL_SEC", Some("600")),
                ("TE_OIDC_JWKS_REFRESH_SEC", Some("120")),
            ],
            || {
                let cfg = read_auth_config_from_env().unwrap();
                let AuthConfig::Enabled(c) = cfg else {
                    panic!("expected Enabled");
                };
                assert_eq!(c.issuer, "https://idp.example.com");
                assert_eq!(c.audiences, vec!["api", "dashboard"]);
                assert_eq!(c.basic_audience.as_deref(), Some("api"));
                assert_eq!(c.basic_scope.as_deref(), Some("jobs.read"));
                assert_eq!(c.basic_cache_ttl, Duration::from_secs(600));
                assert_eq!(c.jwks_refresh, Duration::from_secs(120));
            },
        );
    }
}
