//! Basic → JWT credential exchanger.
//!
//! Translates `Authorization: Basic <client_id:client_secret>` into a JWT by
//! calling the IdP's `client_credentials` grant. Caches successful exchanges
//! until the JWT's expiry (capped), single-flights concurrent exchanges for
//! the same `client_id` to prevent IdP-thrash, and negative-caches IdP
//! rejections for 30 s and transient failures for 5 s.

use crate::AuthError;
use dashmap::DashMap;
use sha2::{Digest, Sha256};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

/// In-memory cache + IdP token-endpoint client for the Basic credential path.
///
/// Cheaply `Clone`-able: the underlying state (caches, in-flight map, HTTP
/// client) is shared via [`Arc`].
#[derive(Clone)]
pub struct BasicExchanger {
    inner: Arc<ExchangerInner>,
}

struct ExchangerInner {
    token_endpoint: String,
    audience: Option<String>,
    scope: Option<String>,
    hard_ttl: Duration,
    positive: DashMap<CacheKey, CachedToken>,
    negative: DashMap<CacheKey, NegativeEntry>,
    inflight: DashMap<String, Arc<Mutex<()>>>,
    http: reqwest::Client,
}

#[derive(PartialEq, Eq, Hash, Clone)]
struct CacheKey {
    client_id: String,
    secret_hash: [u8; 32],
}

struct CachedToken {
    jwt: String,
    expires_at: Instant,
}

struct NegativeEntry {
    until: Instant,
    error: NegativeReason,
}

#[derive(Clone, Copy)]
enum NegativeReason {
    Rejected,
    Transient,
}

#[derive(serde::Deserialize)]
struct TokenResponse {
    access_token: String,
    #[serde(default)]
    expires_in: Option<u64>,
}

/// Build a `reqwest::Client` with a 10 s timeout — matches the precedent
/// established by `Validator` so a stalled IdP can never hang discovery or a
/// token exchange indefinitely.
fn build_http_client() -> Result<reqwest::Client, AuthError> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|e| AuthError::IdpUnreachable(format!("http client: {e}")))
}

impl BasicExchanger {
    /// Build by performing OIDC discovery to locate the token endpoint.
    ///
    /// `audience` and `scope`, if provided, are added to every
    /// `client_credentials` POST. `hard_ttl` caps positive cache entries even
    /// when the IdP returns a longer `expires_in`.
    pub async fn new(
        issuer: String,
        audience: Option<String>,
        scope: Option<String>,
        hard_ttl: Duration,
    ) -> Result<Self, AuthError> {
        let http = build_http_client()?;
        let discovery_url = format!(
            "{}/.well-known/openid-configuration",
            issuer.trim_end_matches('/')
        );
        let resp: serde_json::Value = http
            .get(&discovery_url)
            .send()
            .await
            .map_err(|e| AuthError::IdpUnreachable(format!("discovery: {e}")))?
            .json()
            .await
            .map_err(|e| AuthError::IdpMalformedResponse(format!("discovery: {e}")))?;
        let token_endpoint = resp
            .get("token_endpoint")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                AuthError::IdpMalformedResponse("discovery missing token_endpoint".into())
            })?
            .to_string();
        Ok(Self {
            inner: Arc::new(ExchangerInner {
                token_endpoint,
                audience,
                scope,
                hard_ttl,
                positive: DashMap::new(),
                negative: DashMap::new(),
                inflight: DashMap::new(),
                http: build_http_client()?,
            }),
        })
    }

    /// Exchange Basic credentials for an access-token JWT.
    ///
    /// Hits the positive cache first, then the negative cache, then
    /// single-flights an IdP token request keyed by `client_id`. On success
    /// the JWT is cached until the earlier of `expires_in - 60s` and
    /// `hard_ttl - 60s`. IdP 4xx rejections are negative-cached for 30 s;
    /// transient failures (network / 5xx) for 5 s.
    pub async fn exchange(&self, client_id: &str, secret: &str) -> Result<String, AuthError> {
        let key = CacheKey {
            client_id: client_id.to_string(),
            secret_hash: hash_secret(secret),
        };

        if let Some(cached) = self.inner.positive.get(&key) {
            if cached.expires_at > Instant::now() {
                return Ok(cached.jwt.clone());
            }
        }
        if let Some(neg) = self.inner.negative.get(&key) {
            if neg.until > Instant::now() {
                return Err(match neg.error {
                    NegativeReason::Rejected => AuthError::IdpRejected,
                    NegativeReason::Transient => {
                        AuthError::IdpUnreachable("recent transient failure".into())
                    }
                });
            }
        }

        let lock = self
            .inner
            .inflight
            .entry(client_id.to_string())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone();
        let _guard = lock.lock().await;

        if let Some(cached) = self.inner.positive.get(&key) {
            if cached.expires_at > Instant::now() {
                return Ok(cached.jwt.clone());
            }
        }

        let result = self.exchange_inner(client_id, secret).await;
        match &result {
            Ok((jwt, expires_in)) => {
                let ttl = expires_in
                    .map(Duration::from_secs)
                    .unwrap_or(self.inner.hard_ttl)
                    .min(self.inner.hard_ttl)
                    .saturating_sub(Duration::from_secs(60));
                self.inner.positive.insert(
                    key.clone(),
                    CachedToken {
                        jwt: jwt.clone(),
                        expires_at: Instant::now() + ttl,
                    },
                );
                self.inner.negative.remove(&key);
            }
            Err(AuthError::IdpRejected) => {
                self.inner.negative.insert(
                    key,
                    NegativeEntry {
                        until: Instant::now() + Duration::from_secs(30),
                        error: NegativeReason::Rejected,
                    },
                );
            }
            Err(AuthError::IdpUnreachable(_)) => {
                self.inner.negative.insert(
                    key,
                    NegativeEntry {
                        until: Instant::now() + Duration::from_secs(5),
                        error: NegativeReason::Transient,
                    },
                );
            }
            _ => {}
        }
        result.map(|(jwt, _)| jwt)
    }

    /// Flush cache entries. `client_id = None` flushes all.
    /// Returns `(positive_evicted, negative_evicted)`.
    pub fn flush(&self, client_id: Option<&str>) -> (usize, usize) {
        let pos_before = self.inner.positive.len();
        let neg_before = self.inner.negative.len();
        if let Some(cid) = client_id {
            self.inner.positive.retain(|k, _| k.client_id != cid);
            self.inner.negative.retain(|k, _| k.client_id != cid);
        } else {
            self.inner.positive.clear();
            self.inner.negative.clear();
        }
        (
            pos_before - self.inner.positive.len(),
            neg_before - self.inner.negative.len(),
        )
    }

    async fn exchange_inner(
        &self,
        client_id: &str,
        secret: &str,
    ) -> Result<(String, Option<u64>), AuthError> {
        let mut form: Vec<(&str, &str)> = vec![
            ("grant_type", "client_credentials"),
            ("client_id", client_id),
            ("client_secret", secret),
        ];
        if let Some(a) = &self.inner.audience {
            form.push(("audience", a));
        }
        if let Some(s) = &self.inner.scope {
            form.push(("scope", s));
        }

        let resp = self
            .inner
            .http
            .post(&self.inner.token_endpoint)
            .form(&form)
            .send()
            .await
            .map_err(|e| AuthError::IdpUnreachable(format!("token POST: {e}")))?;

        let status = resp.status();
        if status == reqwest::StatusCode::UNAUTHORIZED
            || status == reqwest::StatusCode::FORBIDDEN
            || status == reqwest::StatusCode::BAD_REQUEST
        {
            return Err(AuthError::IdpRejected);
        }
        if !status.is_success() {
            return Err(AuthError::IdpUnreachable(format!(
                "token endpoint status {status}"
            )));
        }
        let body: TokenResponse = resp
            .json()
            .await
            .map_err(|e| AuthError::IdpMalformedResponse(format!("token body: {e}")))?;
        Ok((body.access_token, body.expires_in))
    }
}

fn hash_secret(secret: &str) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(secret.as_bytes());
    hasher.finalize().into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, Respond, ResponseTemplate};

    struct CountingResponder {
        count: Arc<AtomicUsize>,
        body: serde_json::Value,
        status: u16,
    }
    impl Respond for CountingResponder {
        fn respond(&self, _req: &wiremock::Request) -> ResponseTemplate {
            self.count.fetch_add(1, Ordering::SeqCst);
            ResponseTemplate::new(self.status).set_body_json(&self.body)
        }
    }

    async fn make_idp(
        token_response: serde_json::Value,
        status: u16,
    ) -> (MockServer, Arc<AtomicUsize>) {
        let server = MockServer::start().await;
        let issuer = server.uri();
        Mock::given(method("GET"))
            .and(path("/.well-known/openid-configuration"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "issuer": issuer,
                "jwks_uri": format!("{issuer}/jwks.json"),
                "token_endpoint": format!("{issuer}/token"),
                "authorization_endpoint": format!("{issuer}/authorize"),
                "response_types_supported": ["code"],
                "subject_types_supported": ["public"],
                "id_token_signing_alg_values_supported": ["RS256"],
            })))
            .mount(&server)
            .await;
        let count = Arc::new(AtomicUsize::new(0));
        Mock::given(method("POST"))
            .and(path("/token"))
            .respond_with(CountingResponder {
                count: count.clone(),
                body: token_response,
                status,
            })
            .mount(&server)
            .await;
        (server, count)
    }

    #[tokio::test]
    async fn caches_successful_exchange() {
        let (server, count) = make_idp(
            serde_json::json!({"access_token": "jwt-1", "expires_in": 3600}),
            200,
        )
        .await;
        let exchanger = BasicExchanger::new(
            server.uri(),
            Some("api".into()),
            None,
            Duration::from_secs(3600),
        )
        .await
        .unwrap();
        let t1 = exchanger.exchange("cid", "sec").await.unwrap();
        let t2 = exchanger.exchange("cid", "sec").await.unwrap();
        assert_eq!(t1, "jwt-1");
        assert_eq!(t2, "jwt-1");
        assert_eq!(count.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn single_flights_concurrent_exchanges() {
        let (server, count) = make_idp(
            serde_json::json!({"access_token": "jwt-1", "expires_in": 3600}),
            200,
        )
        .await;
        let exchanger = BasicExchanger::new(server.uri(), None, None, Duration::from_secs(3600))
            .await
            .unwrap();
        let mut handles = vec![];
        for _ in 0..20 {
            let e = exchanger.clone();
            handles.push(tokio::spawn(async move { e.exchange("cid", "sec").await }));
        }
        for h in handles {
            assert_eq!(h.await.unwrap().unwrap(), "jwt-1");
        }
        assert_eq!(count.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn negative_caches_idp_rejection() {
        let (server, count) =
            make_idp(serde_json::json!({"error": "invalid_client"}), 401).await;
        let exchanger = BasicExchanger::new(server.uri(), None, None, Duration::from_secs(3600))
            .await
            .unwrap();
        let _ = exchanger.exchange("cid", "bad").await.unwrap_err();
        let _ = exchanger.exchange("cid", "bad").await.unwrap_err();
        assert_eq!(count.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn flush_evicts_entries() {
        let (server, count) = make_idp(
            serde_json::json!({"access_token": "jwt-1", "expires_in": 3600}),
            200,
        )
        .await;
        let exchanger = BasicExchanger::new(server.uri(), None, None, Duration::from_secs(3600))
            .await
            .unwrap();
        exchanger.exchange("cid", "sec").await.unwrap();
        let (pos, neg) = exchanger.flush(Some("cid"));
        assert_eq!(pos, 1);
        assert_eq!(neg, 0);
        exchanger.exchange("cid", "sec").await.unwrap();
        assert_eq!(count.load(Ordering::SeqCst), 2);
    }
}
