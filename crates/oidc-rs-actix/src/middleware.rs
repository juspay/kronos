//! Actix `Transform` that authenticates each request against the configured
//! [`oidc_rs::AuthConfig`] and injects [`oidc_rs::Identity`] into the request
//! extensions.

use actix_web::body::BoxBody;
use actix_web::dev::{forward_ready, Service, ServiceRequest, ServiceResponse, Transform};
use actix_web::{Error, HttpMessage};
use base64::Engine as _;
use oidc_rs::{AuthError, BasicExchanger, Identity, Validator};
use std::future::{self, Future, Ready};
use std::pin::Pin;
use std::rc::Rc;
use std::sync::Arc;

use crate::error::to_response;

/// Auth middleware "factory". Configure once at startup and wrap any scope.
///
/// The held [`AuthState`] is `Arc`-shared so a single instance can be cloned
/// across `HttpServer` workers — the underlying [`oidc_rs::Validator`] and
/// [`oidc_rs::BasicExchanger`] are themselves `Arc`-backed, so per-worker
/// clones share the same JWKS cache, refresh task, and credential-exchange
/// cache.
#[derive(Clone)]
pub struct AuthMiddleware {
    /// Shared auth state.
    pub state: Arc<AuthState>,
}

/// Shared, runtime-built state. Holds the configured mode.
#[derive(Clone)]
pub struct AuthState {
    /// Auth mode: disabled (synthetic [`Identity::Disabled`]) or
    /// enabled with a [`Validator`] + [`BasicExchanger`].
    pub mode: AuthMode,
}

/// Auth mode discriminator.
#[derive(Clone)]
pub enum AuthMode {
    /// Bypass all auth.
    Disabled,
    /// Validate inbound credentials.
    Enabled {
        /// JWT validator.
        validator: Validator,
        /// Basic→JWT exchanger.
        exchanger: BasicExchanger,
    },
}

impl AuthState {
    /// Borrow the underlying exchanger, if any. Useful for cache-flush
    /// endpoints.
    pub fn exchanger(&self) -> Option<&BasicExchanger> {
        match &self.mode {
            AuthMode::Enabled { exchanger, .. } => Some(exchanger),
            AuthMode::Disabled => None,
        }
    }
}

impl<S, B> Transform<S, ServiceRequest> for AuthMiddleware
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error> + 'static,
    S::Future: 'static,
    B: actix_web::body::MessageBody + 'static,
{
    type Response = ServiceResponse<BoxBody>;
    type Error = Error;
    type InitError = ();
    type Transform = AuthMiddlewareService<S>;
    type Future = Ready<Result<Self::Transform, Self::InitError>>;

    fn new_transform(&self, service: S) -> Self::Future {
        future::ready(Ok(AuthMiddlewareService {
            service: Rc::new(service),
            state: self.state.clone(),
        }))
    }
}

#[doc(hidden)]
pub struct AuthMiddlewareService<S> {
    service: Rc<S>,
    state: Arc<AuthState>,
}

impl<S, B> Service<ServiceRequest> for AuthMiddlewareService<S>
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error> + 'static,
    S::Future: 'static,
    B: actix_web::body::MessageBody + 'static,
{
    type Response = ServiceResponse<BoxBody>;
    type Error = Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>>>>;

    forward_ready!(service);

    fn call(&self, req: ServiceRequest) -> Self::Future {
        let service = self.service.clone();
        let state = self.state.clone();
        Box::pin(async move {
            let identity = match resolve_identity(&state, &req).await {
                Ok(id) => id,
                Err(e) => {
                    let response = to_response(&e);
                    return Ok(req
                        .error_response(actix_web::error::InternalError::from_response(
                            "auth", response,
                        ))
                        .map_into_boxed_body());
                }
            };
            req.extensions_mut().insert(identity);
            service.call(req).await.map(|r| r.map_into_boxed_body())
        })
    }
}

async fn resolve_identity(
    state: &AuthState,
    req: &ServiceRequest,
) -> Result<Identity, AuthError> {
    match &state.mode {
        AuthMode::Disabled => Ok(Identity::Disabled),
        AuthMode::Enabled { validator, exchanger } => {
            let header = req
                .headers()
                .get("authorization")
                .and_then(|v| v.to_str().ok())
                .ok_or(AuthError::MissingHeader)?;
            if let Some(token) = strip_scheme_prefix(header, "Bearer") {
                let claims = validator.validate(token).await?;
                Ok(Identity::Bearer(claims))
            } else if let Some(b64) = strip_scheme_prefix(header, "Basic") {
                let decoded = base64::engine::general_purpose::STANDARD
                    .decode(b64.trim())
                    .map_err(|_| AuthError::MalformedHeader)?;
                let s =
                    std::str::from_utf8(&decoded).map_err(|_| AuthError::MalformedHeader)?;
                let (id, secret) = s.split_once(':').ok_or(AuthError::MalformedHeader)?;
                let jwt = exchanger.exchange(id, secret).await?;
                let claims = validator.validate(&jwt).await?;
                Ok(Identity::Basic(claims))
            } else {
                Err(AuthError::MalformedHeader)
            }
        }
    }
}

/// Strip a case-insensitive auth-scheme prefix (RFC 7235 § 2.1: "Note that
/// both scheme and parameter names are matched case-insensitively…"). The
/// scheme token must be followed by a literal SP separator; surrounding
/// whitespace on the credential is left for the caller to trim.
///
/// Returns `None` when the header doesn't begin with `<scheme> ` regardless
/// of case, so e.g. `"bearer abc"`, `"BEARER abc"`, and `"Bearer abc"` all
/// match `strip_scheme_prefix(header, "Bearer")`, while `"BearerToken abc"`
/// (no space) and `"" / "Bearer"` (too short) do not.
fn strip_scheme_prefix<'a>(header: &'a str, scheme: &str) -> Option<&'a str> {
    let prefix_len = scheme.len();
    if header.len() <= prefix_len {
        return None;
    }
    if !header[..prefix_len].eq_ignore_ascii_case(scheme) {
        return None;
    }
    let rest = &header[prefix_len..];
    if !rest.starts_with(' ') {
        return None;
    }
    Some(rest.trim_start())
}
