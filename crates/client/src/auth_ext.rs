//! Hand-authored extensions to the generated SDK's [`Config`].
//!
//! Codegen does NOT produce this file. The justfile's `smithy-build` recipe
//! restores it from the working tree after each regeneration. See:
//!
//!   * the convenience methods [`Config::with_basic`], [`Config::with_bearer`],
//!     and the deprecated [`Config::with_token`];
//!   * the [`StaticAuthHeaderInterceptor`] which injects the `Authorization`
//!     header at `modify_before_transmit` time.
//!
//! # Why a side file
//!
//! Earlier revisions put these helpers inside the generated `config.rs`. The
//! Smithy codegen wipes `crates/client/` on every regen, so the helpers
//! vanished. Keeping them in `auth_ext.rs` (a file the generator does not
//! produce) means the codegen wipe leaves them untouched on disk; the
//! justfile then only needs to re-add the `pub mod auth_ext;` line in
//! `lib.rs` and the `base64` dep in `Cargo.toml`.
//!
//! # Why `modify_before_transmit` instead of `modify_before_signing`
//!
//! Every operation registers `HTTP_BEARER_AUTH_SCHEME_ID` in its auth
//! options. The smithy orchestrator resolves the configured identity BEFORE
//! `modify_before_signing` runs; `BearerAuthSigner::sign_http_request` then
//! runs AFTER `modify_before_signing` and would overwrite any
//! `Authorization` header set there with its own `Bearer` value.
//!
//! Using `modify_before_transmit` — which fires AFTER signing and
//! immediately before the request leaves the client — guarantees our Basic
//! header is the one that hits the wire. See the smoke tests in
//! `tests/basic_auth_smoke.rs` for the assertion.

use crate::Config;
use base64::Engine as _;

/// An interceptor that injects a pre-computed `Authorization` header value
/// on every outbound request. Used internally by [`Config::with_basic`].
///
/// To make identity resolution succeed when only `with_basic` is configured
/// (and no real bearer token is set), `with_basic` also installs a
/// placeholder bearer token. The placeholder's `Bearer ` value is written
/// during signing and then overwritten by this interceptor before transmit.
#[derive(Debug, Clone)]
struct StaticAuthHeaderInterceptor {
    header_value: ::std::string::String,
}

impl ::aws_smithy_runtime_api::client::interceptors::Intercept for StaticAuthHeaderInterceptor {
    fn name(&self) -> &'static str {
        "StaticAuthHeaderInterceptor"
    }

    fn modify_before_transmit(
        &self,
        context: &mut ::aws_smithy_runtime_api::client::interceptors::context::BeforeTransmitInterceptorContextMut<'_>,
        _runtime_components: &::aws_smithy_runtime_api::client::runtime_components::RuntimeComponents,
        _cfg: &mut ::aws_smithy_types::config_bag::ConfigBag,
    ) -> ::std::result::Result<(), ::aws_smithy_runtime_api::box_error::BoxError> {
        context
            .request_mut()
            .headers_mut()
            .insert("Authorization", self.header_value.clone());
        Ok(())
    }
}

impl Config {
    /// Configure HTTP Basic authentication.
    ///
    /// The Kronos API accepts Basic credentials and exchanges them with the
    /// configured IdP's `client_credentials` grant on the server side; the
    /// resulting JWT is cached for the token's lifetime. The caller only
    /// needs to supply the `client_id` and `client_secret` — no token
    /// management is required.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// let config = kronos_sdk::Config::builder()
    ///     .endpoint_url("https://api.example.com")
    ///     .behavior_version(kronos_sdk::config::BehaviorVersion::latest())
    ///     .build()
    ///     .with_basic("my-client-id", "my-client-secret");
    /// let client = kronos_sdk::Client::from_conf(config);
    /// ```
    pub fn with_basic(
        self,
        client_id: impl ::std::convert::Into<::std::string::String>,
        client_secret: impl ::std::convert::Into<::std::string::String>,
    ) -> Self {
        let raw = ::std::format!("{}:{}", client_id.into(), client_secret.into());
        let b64 = base64::engine::general_purpose::STANDARD.encode(raw);
        let header_value = ::std::format!("Basic {b64}");
        // Install a placeholder bearer token so the smithy orchestrator's
        // identity resolution succeeds. Without this, every operation's
        // auth-options list (which advertises only HTTP_BEARER_AUTH_SCHEME_ID)
        // cannot find a matching identity and the orchestrator fails with
        // `NoMatchingAuthSchemeError` BEFORE our interceptor ever runs.
        // The placeholder's `Bearer ` header is written during signing and
        // then overwritten by `StaticAuthHeaderInterceptor::modify_before_transmit`.
        self.to_builder()
            .bearer_token(::aws_smithy_runtime_api::client::identity::http::Token::new(
                "x", None,
            ))
            .interceptor(StaticAuthHeaderInterceptor { header_value })
            .build()
    }

    /// Configure HTTP Bearer authentication with an externally-obtained JWT.
    ///
    /// Pass a token obtained from your IdP's `client_credentials` or
    /// `authorization_code` flow. The token is sent as
    /// `Authorization: Bearer <token>` on every request.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// let config = kronos_sdk::Config::builder()
    ///     .endpoint_url("https://api.example.com")
    ///     .behavior_version(kronos_sdk::config::BehaviorVersion::latest())
    ///     .build()
    ///     .with_bearer("eyJhbGci...");
    /// let client = kronos_sdk::Client::from_conf(config);
    /// ```
    pub fn with_bearer(self, token: impl ::std::convert::Into<::std::string::String>) -> Self {
        self.to_builder()
            .bearer_token(::aws_smithy_runtime_api::client::identity::http::Token::new(
                token.into(),
                None,
            ))
            .build()
    }

    /// Deprecated alias of [`Self::with_bearer`].
    ///
    /// Use [`with_bearer`](Self::with_bearer) instead.
    #[deprecated(note = "use with_bearer")]
    pub fn with_token(self, token: impl ::std::convert::Into<::std::string::String>) -> Self {
        self.with_bearer(token)
    }
}
