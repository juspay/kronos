//! Core auth types: the validated [`Claims`] payload, the [`Identity`]
//! variant tag (named after the credential format that produced it), and
//! the [`AuthError`] type returned by validators and middleware.

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// JWT claims extracted from a validated token, regardless of the credential
/// format that produced it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Claims {
    /// `iss` — token issuer.
    pub iss: String,
    /// `sub` — the subject the token was issued to.
    pub sub: String,
    /// `email` (OIDC), if present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    /// `name` (OIDC), if present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// `scope` (RFC 6749, space-separated) or `scp` (Azure AD, array) — both
    /// normalised to a Vec. An empty Vec means "no scopes were asserted by
    /// the token" (a typical case for human-interactive ID tokens), not an
    /// error condition.
    #[serde(default)]
    pub scopes: Vec<String>,
}

/// The authenticated identity attached to a request, named after the
/// credential format that produced it.
///
/// The naming is deliberate: nothing about who the caller is is
/// structurally guaranteed. A backend service that does its own
/// `client_credentials` exchange and presents the JWT directly arrives on
/// the [`Identity::Bearer`] path, and the library does not try to relabel
/// it as "M2M" — claim conventions for distinguishing service tokens from
/// interactive ones vary across IdPs.
///
/// Note: `Serialize` only — not `Deserialize`. `Identity` is produced by
/// the validator after verifying a JWT against the IdP's JWKS, and never
/// reconstructed from untrusted JSON. Adding `Deserialize` would let
/// downstream code accidentally trust a forwarded JSON blob as if it had
/// been validated.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Identity {
    /// Auth is disabled at the middleware layer (e.g. local dev).
    Disabled,
    /// Arrived as `Authorization: Bearer <jwt>`.
    Bearer(Claims),
    /// Arrived as `Authorization: Basic …`, exchanged for a JWT against the
    /// IdP's `client_credentials` grant, then validated.
    Basic(Claims),
}

/// Errors surfaced by the auth pipeline.
#[derive(Debug, Error)]
pub enum AuthError {
    /// No `Authorization` header on the request.
    #[error("missing Authorization header")]
    MissingHeader,
    /// `Authorization` header could not be parsed.
    #[error("malformed Authorization header")]
    MalformedHeader,
    /// JWT `exp` is in the past (beyond skew tolerance).
    #[error("token expired")]
    Expired,
    /// JWT signature did not verify against the issuer's JWKS.
    #[error("token signature invalid")]
    BadSignature,
    /// JWT `iss` did not match the configured issuer.
    #[error("token issuer not accepted: {0}")]
    BadIssuer(String),
    /// JWT `aud` did not match any of the configured audiences.
    #[error("token audience not accepted")]
    BadAudience,
    /// IdP did not respond / was unreachable / returned 5xx.
    #[error("identity provider unreachable: {0}")]
    IdpUnreachable(String),
    /// IdP returned a 4xx during token exchange.
    #[error("identity provider rejected credentials")]
    IdpRejected,
    /// IdP returned a response we couldn't parse.
    #[error("identity provider returned malformed response: {0}")]
    IdpMalformedResponse(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_serializes_with_discriminator_and_claims() {
        let claims = Claims {
            iss: "https://idp.example.com".into(),
            sub: "user-123".into(),
            email: Some("alice@example.com".into()),
            name: Some("Alice".into()),
            scopes: vec![],
        };
        let id = Identity::Bearer(claims);
        let json = serde_json::to_value(&id).unwrap();
        assert_eq!(json["type"], "bearer");
        assert_eq!(json["sub"], "user-123");
        assert_eq!(json["email"], "alice@example.com");
        assert!(json.get("scopes").is_some());
    }

    #[test]
    fn identity_disabled_serializes_with_just_type() {
        let json = serde_json::to_value(&Identity::Disabled).unwrap();
        assert_eq!(json, serde_json::json!({"type": "disabled"}));
    }

    #[test]
    fn claims_email_and_name_omitted_when_none() {
        let claims = Claims {
            iss: "https://idp".into(),
            sub: "service".into(),
            email: None,
            name: None,
            scopes: vec!["jobs.read".into()],
        };
        let id = Identity::Basic(claims);
        let json = serde_json::to_value(&id).unwrap();
        assert_eq!(json["type"], "basic");
        assert!(json.get("email").is_none());
        assert!(json.get("name").is_none());
        assert_eq!(json["scopes"], serde_json::json!(["jobs.read"]));
    }

    #[test]
    fn auth_error_display_messages_include_inner_strings() {
        assert_eq!(
            AuthError::MissingHeader.to_string(),
            "missing Authorization header"
        );
        assert_eq!(
            AuthError::BadIssuer("https://wrong.example.com".into()).to_string(),
            "token issuer not accepted: https://wrong.example.com"
        );
        assert_eq!(
            AuthError::IdpUnreachable("dns lookup failed".into()).to_string(),
            "identity provider unreachable: dns lookup failed"
        );
    }
}
