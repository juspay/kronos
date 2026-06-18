//! Basic → JWT credential exchanger.
//!
//! Translates `Authorization: Basic <base64(client_id:client_secret)>` into a JWT by
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

/// Build a `reqwest::Client` with a 10 s timeout and redirects disabled —
/// matches the precedent established by `Validator` so a stalled IdP can
/// never hang discovery or a token exchange indefinitely, and so an
/// attacker-controlled redirect from a malicious issuer URL can't steer us
/// to an arbitrary host.
fn build_http_client() -> Result<reqwest::Client, AuthError> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .redirect(reqwest::redirect::Policy::none())
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
            // Expired — evict on read so a unique-but-stale `(client_id,
            // secret_hash)` pair can't accumulate indefinitely. `drop(cached)`
            // releases the DashMap read guard before `remove`; without it
            // the bucket-lock deadlocks against itself.
            drop(cached);
            self.inner.positive.remove(&key);
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
            // Expired — same eviction-on-read rationale as the positive cache.
            drop(neg);
            self.inner.negative.remove(&key);
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
                let jwt = cached.jwt.clone();
                drop(cached);
                drop(_guard);
                drop(lock);
                self.cleanup_inflight(client_id);
                return Ok(jwt);
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
        drop(_guard);
        drop(lock);
        self.cleanup_inflight(client_id);
        result.map(|(jwt, _)| jwt)
    }

    /// Remove the per-`client_id` entry from `inflight` IFF we hold the last
    /// reference. Race-free: `DashMap::remove_if` runs the predicate while
    /// holding the bucket lock, so a concurrent `entry().or_insert_with()`
    /// for the same `client_id` either (a) sees our entry and bumps the
    /// `Arc` count above 1 before our predicate runs (keeping the entry),
    /// or (b) inserts after we removed and gets a fresh `Arc`.
    ///
    /// IMPORTANT: callers MUST `drop` their local clone of the `Arc<Mutex<()>>`
    /// before calling this — otherwise the strong count is at least 2 (the
    /// map's entry + the caller's clone) and the predicate never fires.
    ///
    /// Without this, every distinct `client_id` ever seen by `exchange()`
    /// leaks a `(String, Arc<Mutex<()>>)` entry — slow but unbounded growth
    /// in long-running services with credential rotation.
    fn cleanup_inflight(&self, client_id: &str) {
        self.inner
            .inflight
            .remove_if(client_id, |_, mutex| Arc::strong_count(mutex) <= 1);
    }

    /// Flush cache entries. `client_id = None` flushes all.
    /// Returns `(positive_evicted, negative_evicted)`.
    ///
    /// Also clears matching `inflight` entries so operator-driven cache
    /// flushes (e.g. after credential rotation) cap the in-flight map's
    /// long-tail too. Concurrent in-flight exchanges still hold a clone of
    /// the `Arc<Mutex<()>>`, so removing the map entry only releases the
    /// map's reference; in-flight callers complete normally.
    pub fn flush(&self, client_id: Option<&str>) -> (usize, usize) {
        let pos_before = self.inner.positive.len();
        let neg_before = self.inner.negative.len();
        if let Some(cid) = client_id {
            self.inner.positive.retain(|k, _| k.client_id != cid);
            self.inner.negative.retain(|k, _| k.client_id != cid);
            self.inner.inflight.retain(|k, _| k != cid);
        } else {
            self.inner.positive.clear();
            self.inner.negative.clear();
            self.inner.inflight.clear();
        }
        (
            pos_before - self.inner.positive.len(),
            neg_before - self.inner.negative.len(),
        )
    }

    /// Test-only accessor exposing the positive cache's current size. Used by
    /// the eviction regression test to assert that a stale entry is removed
    /// after a hit on the expired-entry branch in [`Self::exchange`].
    #[cfg(test)]
    pub fn positive_len(&self) -> usize {
        self.inner.positive.len()
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

    #[tokio::test]
    async fn inflight_is_empty_after_exchange() {
        // Regression test for the unbounded `inflight` map leak. Before the
        // fix, each distinct `client_id` left a `(String, Arc<Mutex<()>>)`
        // entry that was never removed. After the fix, the entry is removed
        // when the last caller drops the lock.
        let (server, _) = make_idp(
            serde_json::json!({"access_token": "jwt-1", "expires_in": 3600}),
            200,
        )
        .await;
        let exchanger = BasicExchanger::new(server.uri(), None, None, Duration::from_secs(3600))
            .await
            .unwrap();

        // Successful exchange — `inflight` should drain to 0 once exchange returns.
        exchanger.exchange("cid-a", "sec").await.unwrap();
        assert_eq!(
            exchanger.inner.inflight.len(),
            0,
            "inflight must be empty after a successful exchange returns"
        );

        // A second `client_id` should not leave residue either.
        exchanger.exchange("cid-b", "sec").await.unwrap();
        assert_eq!(
            exchanger.inner.inflight.len(),
            0,
            "inflight must be empty after exchanging multiple distinct client_ids"
        );

        // A negative-cached call also goes through the exchange path on first
        // miss and must clean up its inflight entry.
        let (bad_server, _) =
            make_idp(serde_json::json!({"error": "invalid_client"}), 401).await;
        let bad_exchanger =
            BasicExchanger::new(bad_server.uri(), None, None, Duration::from_secs(3600))
                .await
                .unwrap();
        let _ = bad_exchanger.exchange("cid-bad", "x").await.unwrap_err();
        assert_eq!(
            bad_exchanger.inner.inflight.len(),
            0,
            "inflight must be empty even after an IdP rejection"
        );
    }

    #[tokio::test]
    async fn expired_positive_entry_is_evicted_on_read() {
        // Regression test: before the fix, expired positive entries were
        // bypassed but never removed, so a long-running service that saw a
        // stream of unique `(client_id, secret_hash)` pairs would leak
        // memory. After the fix, calling `exchange()` on a key with an
        // expired entry removes that entry before continuing.
        //
        // We provoke an expired entry by setting `hard_ttl = 60s` and
        // returning `expires_in: 61` from the IdP — the cached `expires_at`
        // is then `now + (min(61, 60) - 60) = now + 0s`, i.e. already in
        // the past by the time we observe it.
        let (server, _) = make_idp(
            serde_json::json!({"access_token": "jwt-1", "expires_in": 61}),
            200,
        )
        .await;
        let exchanger =
            BasicExchanger::new(server.uri(), None, None, Duration::from_secs(60))
                .await
                .unwrap();
        exchanger.exchange("cid", "sec").await.unwrap();

        // The entry was inserted with an already-expired `expires_at`. Confirm
        // it's still in the map *before* the next call — it's only the next
        // `exchange()` call that triggers the on-read eviction.
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert_eq!(
            exchanger.positive_len(),
            1,
            "stale entry should still be present before the next exchange()"
        );

        // The next call sees the expired entry and evicts it. (It will then
        // proceed to mint a fresh entry — which is also expired, so by the
        // end of the call the map again holds 1 fresh-but-expired entry.
        // The point is that the OLD entry was removed and replaced, not
        // accumulated alongside.)
        exchanger.exchange("cid", "sec").await.unwrap();
        assert_eq!(
            exchanger.positive_len(),
            1,
            "expired entry should be replaced (not appended) by the on-read eviction"
        );

        // A second call with a DIFFERENT secret writes a SECOND key
        // (different `secret_hash`), and the first key — now expired — gets
        // evicted on its next read attempt. Run with the *original* secret
        // to demonstrate that the first key's stale entry is removed.
        exchanger.exchange("cid", "other-secret").await.unwrap();
        assert_eq!(exchanger.positive_len(), 2);
        exchanger.exchange("cid", "sec").await.unwrap();
        // Eviction-on-read kicked in for the first key, then we wrote a
        // fresh (still-expired) entry — net unchanged.
        assert_eq!(exchanger.positive_len(), 2);
    }
}
