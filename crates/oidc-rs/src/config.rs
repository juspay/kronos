//! Auth configuration. The library exposes a builder; downstream crates
//! decide how to populate it (env, config file, CLI flags, etc.).

use std::time::Duration;

/// Top-level configuration. Use [`AuthConfig::disabled`] for local-dev /
/// no-auth modes, or [`AuthConfig::builder`] to build an enabled config.
#[derive(Debug, Clone)]
pub enum AuthConfig {
    /// Disable authentication entirely. Middleware short-circuits and
    /// returns [`crate::Identity::Disabled`] for every request.
    Disabled,
    /// Validate inbound JWTs against the configured OIDC issuer.
    Enabled(EnabledConfig),
}

/// Strongly-typed configuration for an enabled auth pipeline.
#[derive(Debug, Clone)]
pub struct EnabledConfig {
    /// OIDC issuer URL. Discovery is performed at `<issuer>/.well-known/openid-configuration`.
    pub issuer: String,
    /// `aud` values accepted on inbound JWTs.
    pub audiences: Vec<String>,
    /// `audience` value to request during Basic→JWT exchange (Auth0-style IdPs).
    pub basic_audience: Option<String>,
    /// `scope` to request during Basic→JWT exchange.
    pub basic_scope: Option<String>,
    /// Hard cap on the Basic→JWT cache lifetime.
    pub basic_cache_ttl: Duration,
    /// Interval at which the JWKS is refetched in the background.
    pub jwks_refresh: Duration,
}

impl AuthConfig {
    /// Disabled-mode constructor.
    pub fn disabled() -> Self {
        AuthConfig::Disabled
    }

    /// Start an enabled-mode builder.
    pub fn builder() -> AuthConfigBuilder {
        AuthConfigBuilder::default()
    }
}

/// Builder for [`AuthConfig::Enabled`].
#[derive(Debug, Default)]
pub struct AuthConfigBuilder {
    issuer: Option<String>,
    audiences: Vec<String>,
    basic_audience: Option<String>,
    basic_scope: Option<String>,
    basic_cache_ttl: Option<Duration>,
    jwks_refresh: Option<Duration>,
}

impl AuthConfigBuilder {
    /// REQUIRED — set the OIDC issuer.
    pub fn issuer(mut self, issuer: impl Into<String>) -> Self {
        self.issuer = Some(issuer.into());
        self
    }
    /// REQUIRED — set the list of accepted `aud` values.
    pub fn audiences<I, S>(mut self, audiences: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.audiences = audiences.into_iter().map(Into::into).collect();
        self
    }
    /// Optional — set the `audience` parameter for Basic→JWT exchange.
    pub fn basic_audience(mut self, audience: impl Into<String>) -> Self {
        self.basic_audience = Some(audience.into());
        self
    }
    /// Optional — set the `scope` parameter for Basic→JWT exchange.
    pub fn basic_scope(mut self, scope: impl Into<String>) -> Self {
        self.basic_scope = Some(scope.into());
        self
    }
    /// Override the default 1-hour cache TTL cap.
    pub fn basic_cache_ttl(mut self, ttl: Duration) -> Self {
        self.basic_cache_ttl = Some(ttl);
        self
    }
    /// Override the default 5-minute JWKS refresh interval.
    pub fn jwks_refresh(mut self, interval: Duration) -> Self {
        self.jwks_refresh = Some(interval);
        self
    }
    /// Finish and validate the builder.
    pub fn build(self) -> Result<AuthConfig, BuildError> {
        let issuer = self.issuer.ok_or(BuildError::MissingIssuer)?;
        if self.audiences.is_empty() {
            return Err(BuildError::EmptyAudiences);
        }
        // Reject zero-duration timing values: a zero `jwks_refresh` would
        // later panic `tokio::time::interval`, and a zero `basic_cache_ttl`
        // means every cached entry expires the instant it's written —
        // certainly a configuration error.
        if matches!(self.basic_cache_ttl, Some(t) if t.is_zero()) {
            return Err(BuildError::NonPositiveBasicCacheTtl);
        }
        if matches!(self.jwks_refresh, Some(t) if t.is_zero()) {
            return Err(BuildError::NonPositiveJwksRefresh);
        }
        Ok(AuthConfig::Enabled(EnabledConfig {
            issuer,
            audiences: self.audiences,
            basic_audience: self.basic_audience,
            basic_scope: self.basic_scope,
            basic_cache_ttl: self.basic_cache_ttl.unwrap_or(Duration::from_secs(3600)),
            jwks_refresh: self.jwks_refresh.unwrap_or(Duration::from_secs(300)),
        }))
    }
}

/// Errors returned by [`AuthConfigBuilder::build`].
#[derive(Debug, thiserror::Error)]
pub enum BuildError {
    /// `issuer` was not set.
    #[error("issuer is required")]
    MissingIssuer,
    /// `audiences` was empty or unset.
    #[error("at least one audience is required")]
    EmptyAudiences,
    /// `basic_cache_ttl` was explicitly set to zero.
    #[error("basic cache TTL must be greater than zero")]
    NonPositiveBasicCacheTtl,
    /// `jwks_refresh` was explicitly set to zero (would panic `tokio::time::interval`).
    #[error("JWKS refresh interval must be greater than zero")]
    NonPositiveJwksRefresh,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_constructor() {
        assert!(matches!(AuthConfig::disabled(), AuthConfig::Disabled));
    }

    #[test]
    fn builder_requires_issuer_and_audiences() {
        let err = AuthConfig::builder().build().unwrap_err();
        assert!(matches!(err, BuildError::MissingIssuer));

        let err = AuthConfig::builder()
            .issuer("https://idp.example.com")
            .build()
            .unwrap_err();
        assert!(matches!(err, BuildError::EmptyAudiences));
    }

    #[test]
    fn builder_rejects_zero_durations() {
        let err = AuthConfig::builder()
            .issuer("https://idp.example.com")
            .audiences(["api"])
            .basic_cache_ttl(Duration::ZERO)
            .build()
            .unwrap_err();
        assert!(matches!(err, BuildError::NonPositiveBasicCacheTtl));

        let err = AuthConfig::builder()
            .issuer("https://idp.example.com")
            .audiences(["api"])
            .jwks_refresh(Duration::ZERO)
            .build()
            .unwrap_err();
        assert!(matches!(err, BuildError::NonPositiveJwksRefresh));
    }

    #[test]
    fn builder_full_round_trip() {
        let cfg = AuthConfig::builder()
            .issuer("https://idp.example.com")
            .audiences(["api", "dashboard"])
            .basic_audience("api")
            .basic_scope("jobs.read jobs.write")
            .basic_cache_ttl(Duration::from_secs(600))
            .jwks_refresh(Duration::from_secs(120))
            .build()
            .unwrap();
        match cfg {
            AuthConfig::Enabled(c) => {
                assert_eq!(c.issuer, "https://idp.example.com");
                assert_eq!(c.audiences, vec!["api", "dashboard"]);
                assert_eq!(c.basic_audience.as_deref(), Some("api"));
                assert_eq!(c.basic_scope.as_deref(), Some("jobs.read jobs.write"));
                assert_eq!(c.basic_cache_ttl, Duration::from_secs(600));
                assert_eq!(c.jwks_refresh, Duration::from_secs(120));
            }
            _ => panic!("expected Enabled"),
        }
    }
}
