//! JWT validator backed by an OIDC issuer's JWKS.
//!
//! Discovery (`<issuer>/.well-known/openid-configuration`) is performed at
//! [`Validator::new`] to locate the JWKS URI; the JWKS is fetched once at
//! construction and refreshed in the background at the configured interval.

use crate::{AuthError, Claims};
use openidconnect::core::{CoreJsonWebKeySet, CoreJsonWebKeyUse, CoreProviderMetadata};
use openidconnect::{IssuerUrl, JsonWebKey, JsonWebKeySetUrl};
use serde::Deserialize;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;

/// Validates inbound JWTs against the configured OIDC issuer's JWKS.
///
/// Cheaply `Clone`-able: the underlying state is shared via [`Arc`], so
/// cloning a `Validator` to hand it to multiple request handlers or
/// middleware layers does not duplicate the JWKS cache or HTTP client.
#[derive(Clone)]
pub struct Validator {
    inner: Arc<ValidatorInner>,
}

struct ValidatorInner {
    issuer: String,
    audiences: Vec<String>,
    jwks: RwLock<Option<CoreJsonWebKeySet>>,
    jwks_url: JsonWebKeySetUrl,
    jwks_refresh: Duration,
    http: reqwest::Client,
}

impl Validator {
    /// Discover the issuer, fetch JWKS, and spawn a background refresh task.
    ///
    /// `issuer` is the URL printed as `iss` in tokens (e.g.
    /// `https://idp.example.com`). `audiences` is the list of acceptable
    /// `aud` values for tokens minted for this resource server.
    /// `jwks_refresh` is the interval at which the background task re-fetches
    /// the JWKS. A failed background refresh logs a warning and keeps the
    /// previously-cached JWKS until the next interval.
    pub async fn new(
        issuer: String,
        audiences: Vec<String>,
        jwks_refresh: Duration,
    ) -> Result<Self, AuthError> {
        let http = reqwest::Client::builder()
            // openidconnect-rs recommends disabling redirects to avoid SSRF
            // when discovering arbitrary issuer URLs.
            .redirect(reqwest::redirect::Policy::none())
            // Cap discovery / JWKS-fetch latency so a stalled IdP can't hang
            // `Validator::new` or a background refresh tick indefinitely.
            .timeout(Duration::from_secs(10))
            .build()
            .map_err(|e| AuthError::IdpUnreachable(format!("http client: {e}")))?;

        let issuer_url = IssuerUrl::new(issuer.clone())
            .map_err(|e| AuthError::IdpMalformedResponse(format!("issuer url: {e}")))?;
        let metadata = CoreProviderMetadata::discover_async(issuer_url, &http)
            .await
            .map_err(|e| AuthError::IdpUnreachable(format!("discovery: {e}")))?;
        let jwks_url = metadata.jwks_uri().clone();
        // Best-effort initial JWKS fetch. On failure we log and proceed —
        // the background refresh loop will retry, and `validate()` will return
        // `IdpUnreachable("jwks not loaded yet")` until keys land. We log
        // here so an operator can see *why* keys are missing on startup
        // instead of seeing only the downstream symptom.
        let jwks = match CoreJsonWebKeySet::fetch_async(&jwks_url, &http).await {
            Ok(set) => Some(set),
            Err(e) => {
                tracing::warn!("initial JWKS fetch failed; will retry on refresh tick: {e}");
                None
            }
        };
        let inner = Arc::new(ValidatorInner {
            issuer,
            audiences,
            jwks: RwLock::new(jwks),
            jwks_url,
            jwks_refresh,
            http,
        });
        let bg = inner.clone();
        tokio::spawn(async move { bg.refresh_loop().await });
        Ok(Self { inner })
    }

    /// Validate a raw JWT string. Returns the extracted [`Claims`] on success.
    ///
    /// Performs signature verification against the cached JWKS plus standard
    /// `iss`/`aud`/`exp`/`nbf` checks with a 60-second leeway for clock skew.
    pub async fn validate(&self, raw_jwt: &str) -> Result<Claims, AuthError> {
        let jwks = self
            .inner
            .jwks
            .read()
            .await
            .clone()
            .ok_or_else(|| AuthError::IdpUnreachable("jwks not loaded yet".into()))?;

        let header =
            jsonwebtoken::decode_header(raw_jwt).map_err(|_| AuthError::MalformedHeader)?;
        let kid = header.kid.ok_or(AuthError::MalformedHeader)?;

        let jwk = jwks
            .keys()
            .iter()
            .find(|k| {
                k.key_id()
                    .map(|kid_val| kid_val.as_str() == kid)
                    .unwrap_or(false)
            })
            .ok_or(AuthError::BadSignature)?;

        // openidconnect v4's CoreJsonWebKey hides its RSA/EC parameters
        // behind private fields with no public accessors, so we round-trip
        // through JSON into jsonwebtoken's own Jwk to recover them. The JWK
        // serialisation format is standardised (RFC 7517), so this is safe.
        let jwk_json = serde_json::to_value(jwk)
            .map_err(|_| AuthError::IdpMalformedResponse("jwk serialize".into()))?;
        let jwt_jwk: jsonwebtoken::jwk::Jwk = serde_json::from_value(jwk_json)
            .map_err(|_| AuthError::IdpMalformedResponse("jwk deserialize".into()))?;

        // Defence in depth: refuse keys that the IdP has explicitly published
        // as encryption-only.
        if matches!(jwk.key_use(), Some(CoreJsonWebKeyUse::Encryption)) {
            return Err(AuthError::BadSignature);
        }

        let algorithm = derive_algorithm(&jwt_jwk)?;
        let decoding_key = jsonwebtoken::DecodingKey::from_jwk(&jwt_jwk)
            .map_err(|_| AuthError::BadSignature)?;

        let mut validation = jsonwebtoken::Validation::new(algorithm);
        validation.set_issuer(&[self.inner.issuer.as_str()]);
        validation.set_audience(&self.inner.audiences);
        validation.validate_nbf = true;
        validation.leeway = 60;

        let token = jsonwebtoken::decode::<RawClaims>(raw_jwt, &decoding_key, &validation)
            .map_err(|e| match e.kind() {
                jsonwebtoken::errors::ErrorKind::ExpiredSignature => AuthError::Expired,
                jsonwebtoken::errors::ErrorKind::InvalidAudience => AuthError::BadAudience,
                jsonwebtoken::errors::ErrorKind::InvalidIssuer => {
                    AuthError::BadIssuer(self.inner.issuer.clone())
                }
                _ => AuthError::BadSignature,
            })?;

        Ok(Claims {
            iss: token.claims.iss,
            sub: token.claims.sub,
            email: token.claims.email,
            name: token.claims.name,
            scopes: parse_scopes(token.claims.scope.as_deref(), token.claims.scp.as_deref()),
        })
    }

    /// Force a JWKS refetch. Useful from an admin endpoint when an IdP has
    /// rotated keys and you don't want to wait for the next background tick.
    pub async fn refresh_jwks(&self) -> Result<(), AuthError> {
        let fresh = CoreJsonWebKeySet::fetch_async(&self.inner.jwks_url, &self.inner.http)
            .await
            .map_err(|e| AuthError::IdpUnreachable(format!("jwks refresh: {e}")))?;
        *self.inner.jwks.write().await = Some(fresh);
        Ok(())
    }
}

impl ValidatorInner {
    async fn refresh_loop(self: Arc<Self>) {
        let mut ticker = tokio::time::interval(self.jwks_refresh);
        // The first tick fires immediately; skip it since we just fetched at
        // construction.
        ticker.tick().await;
        loop {
            ticker.tick().await;
            match CoreJsonWebKeySet::fetch_async(&self.jwks_url, &self.http).await {
                Ok(fresh) => *self.jwks.write().await = Some(fresh),
                Err(_) => tracing::warn!("JWKS refresh failed; will retry next interval"),
            }
        }
    }
}

#[derive(Deserialize)]
struct RawClaims {
    iss: String,
    sub: String,
    #[serde(default)]
    email: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    scope: Option<String>,
    #[serde(default)]
    scp: Option<Vec<String>>,
}

fn parse_scopes(scope: Option<&str>, scp: Option<&[String]>) -> Vec<String> {
    if let Some(s) = scope {
        return s.split_whitespace().map(String::from).collect();
    }
    if let Some(arr) = scp {
        return arr.to_vec();
    }
    Vec::new()
}

/// Pick a [`jsonwebtoken::Algorithm`] for verifying with this JWK.
///
/// Prefers the JWK's `alg` field (`KeyAlgorithm`) when present, falling back
/// to a sensible default per key type (RSA → RS256, EC → ES256).
fn derive_algorithm(jwk: &jsonwebtoken::jwk::Jwk) -> Result<jsonwebtoken::Algorithm, AuthError> {
    if let Some(key_alg) = jwk.common.key_algorithm {
        // KeyAlgorithm and Algorithm both implement Display / FromStr around
        // the same JWA names, so a round-trip is the simplest bridge.
        return jsonwebtoken::Algorithm::from_str(&key_alg.to_string())
            .map_err(|_| AuthError::BadSignature);
    }
    match jwk.algorithm {
        jsonwebtoken::jwk::AlgorithmParameters::RSA(_) => Ok(jsonwebtoken::Algorithm::RS256),
        jsonwebtoken::jwk::AlgorithmParameters::EllipticCurve(_) => {
            Ok(jsonwebtoken::Algorithm::ES256)
        }
        _ => Err(AuthError::BadSignature),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jsonwebtoken::{EncodingKey, Header};
    use serde_json::json;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    struct MockIdp {
        server: MockServer,
        encoding_key: EncodingKey,
        kid: String,
    }

    impl MockIdp {
        async fn start() -> Self {
            let priv_pem = include_bytes!("../tests/fixtures/test_rsa_priv.pem");
            let n_b64 = include_str!("../tests/fixtures/test_rsa_n.txt").trim();
            let e_b64 = include_str!("../tests/fixtures/test_rsa_e.txt").trim();
            let server = MockServer::start().await;
            let kid = "test-key-1".to_string();
            let issuer = server.uri();

            Mock::given(method("GET"))
                .and(path("/.well-known/openid-configuration"))
                .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                    "issuer": issuer,
                    "jwks_uri": format!("{issuer}/jwks.json"),
                    "authorization_endpoint": format!("{issuer}/authorize"),
                    "token_endpoint": format!("{issuer}/token"),
                    "response_types_supported": ["code"],
                    "subject_types_supported": ["public"],
                    "id_token_signing_alg_values_supported": ["RS256"],
                })))
                .mount(&server)
                .await;

            Mock::given(method("GET"))
                .and(path("/jwks.json"))
                .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                    "keys": [{
                        "kty": "RSA", "kid": kid, "use": "sig", "alg": "RS256",
                        "n": n_b64, "e": e_b64,
                    }]
                })))
                .mount(&server)
                .await;

            Self {
                server,
                encoding_key: EncodingKey::from_rsa_pem(priv_pem).unwrap(),
                kid,
            }
        }

        fn issuer(&self) -> String {
            self.server.uri()
        }

        fn mint(&self, claims: serde_json::Value) -> String {
            let mut header = Header::new(jsonwebtoken::Algorithm::RS256);
            header.kid = Some(self.kid.clone());
            jsonwebtoken::encode(&header, &claims, &self.encoding_key).unwrap()
        }
    }

    fn now() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
    }

    #[tokio::test]
    async fn validates_a_well_formed_token() {
        let idp = MockIdp::start().await;
        let v = Validator::new(idp.issuer(), vec!["api".into()], Duration::from_secs(300))
            .await
            .unwrap();
        let jwt = idp.mint(json!({
            "iss": idp.issuer(), "sub": "u1", "aud": "api",
            "exp": now() + 300, "iat": now(), "email": "alice@example.com",
        }));
        let claims = v.validate(&jwt).await.unwrap();
        assert_eq!(claims.sub, "u1");
        assert_eq!(claims.email.as_deref(), Some("alice@example.com"));
    }

    #[tokio::test]
    async fn rejects_expired() {
        let idp = MockIdp::start().await;
        let v = Validator::new(idp.issuer(), vec!["api".into()], Duration::from_secs(300))
            .await
            .unwrap();
        let jwt = idp.mint(json!({
            "iss": idp.issuer(), "sub": "u1", "aud": "api",
            "exp": now() - 3600, "iat": now() - 7200,
        }));
        assert!(matches!(
            v.validate(&jwt).await.unwrap_err(),
            AuthError::Expired
        ));
    }

    #[tokio::test]
    async fn rejects_wrong_audience() {
        let idp = MockIdp::start().await;
        let v = Validator::new(idp.issuer(), vec!["api".into()], Duration::from_secs(300))
            .await
            .unwrap();
        let jwt = idp.mint(json!({
            "iss": idp.issuer(), "sub": "u1", "aud": "elsewhere",
            "exp": now() + 300, "iat": now(),
        }));
        assert!(matches!(
            v.validate(&jwt).await.unwrap_err(),
            AuthError::BadAudience
        ));
    }
}
