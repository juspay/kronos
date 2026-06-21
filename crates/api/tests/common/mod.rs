use actix_web::body::BoxBody;
use actix_web::dev::Service;
use actix_web::{web, App};
use kronos_api::middleware::{AuthMiddleware, AuthMode, AuthState};
use oidc_rs::{BasicExchanger, Validator};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// A wiremock-backed mock OIDC issuer. Serves discovery + JWKS by default;
/// call [`install_token_endpoint`] to add a working /token responder for
/// the Basic-credential flow.
pub struct MockIdp {
    pub server: MockServer,
    pub encoding_key: jsonwebtoken::EncodingKey,
    pub kid: String,
    pub token_calls: Arc<AtomicUsize>,
}

impl MockIdp {
    pub async fn start() -> Self {
        let priv_pem = include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/test_rsa_priv.pem"
        ));
        let n_b64 = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/test_rsa_n.txt"
        ))
        .trim();
        let e_b64 = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/test_rsa_e.txt"
        ))
        .trim();

        let server = MockServer::start().await;
        let issuer = server.uri();
        let kid = "test-key-1".to_string();
        let token_calls = Arc::new(AtomicUsize::new(0));

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

        Mock::given(method("GET"))
            .and(path("/jwks.json"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "keys": [{
                    "kty": "RSA", "kid": kid, "use": "sig", "alg": "RS256",
                    "n": n_b64, "e": e_b64,
                }]
            })))
            .mount(&server)
            .await;

        Self {
            server,
            encoding_key: jsonwebtoken::EncodingKey::from_rsa_pem(priv_pem).unwrap(),
            kid,
            token_calls,
        }
    }

    pub fn issuer(&self) -> String {
        self.server.uri()
    }

    /// Mint a JWT signed by the mock IdP. Use to forge access tokens for
    /// the Bearer-path tests.
    pub fn mint(&self, claims: serde_json::Value) -> String {
        let mut header = jsonwebtoken::Header::new(jsonwebtoken::Algorithm::RS256);
        header.kid = Some(self.kid.clone());
        jsonwebtoken::encode(&header, &claims, &self.encoding_key).unwrap()
    }

    /// Install a `/token` responder that successfully exchanges any
    /// client_credentials POST for a JWT with the given audience. Bumps
    /// `token_calls` on each request.
    pub async fn install_token_endpoint(&self, audience: &str) {
        let issuer = self.issuer();
        let kid = self.kid.clone();
        let priv_pem = include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/test_rsa_priv.pem"
        ))
        .to_vec();
        let calls = self.token_calls.clone();
        let aud = audience.to_string();
        Mock::given(method("POST"))
            .and(path("/token"))
            .respond_with(move |_req: &wiremock::Request| {
                calls.fetch_add(1, Ordering::SeqCst);
                let key = jsonwebtoken::EncodingKey::from_rsa_pem(&priv_pem).unwrap();
                let mut header =
                    jsonwebtoken::Header::new(jsonwebtoken::Algorithm::RS256);
                header.kid = Some(kid.clone());
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs();
                let claims = serde_json::json!({
                    "iss": issuer,
                    "sub": "test-m2m-client",
                    "aud": aud,
                    "exp": now + 3600,
                    "iat": now,
                    "scope": "jobs.read jobs.write",
                });
                let jwt = jsonwebtoken::encode(&header, &claims, &key).unwrap();
                ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "access_token": jwt,
                    "token_type": "Bearer",
                    "expires_in": 3600,
                }))
            })
            .mount(&self.server)
            .await;
    }
}

/// Build an in-process app with `TE_AUTH_MODE=disabled` and only the auth
/// routes wired up. Doesn't touch the database — the auth routes don't
/// need it.
pub async fn start_api_disabled() -> impl Service<
    actix_http::Request,
    Response = actix_web::dev::ServiceResponse<BoxBody>,
    Error = actix_web::Error,
> {
    let auth_state = Arc::new(AuthState { mode: AuthMode::Disabled });
    actix_web::test::init_service(
        App::new()
            .app_data(web::Data::from(auth_state.clone()))
            .service(
                web::scope("/v1")
                    .wrap(AuthMiddleware { state: auth_state.clone() })
                    .route(
                        "/auth/whoami",
                        web::get().to(kronos_api::handlers::auth::whoami),
                    )
                    .route(
                        "/auth/cache/flush",
                        web::post().to(kronos_api::handlers::auth::flush_cache),
                    ),
            ),
    )
    .await
}

/// Build an in-process app with auth enabled, pointing at the given mock IdP.
pub async fn start_api_enabled(
    idp: &MockIdp,
    audience: &str,
) -> impl Service<
    actix_http::Request,
    Response = actix_web::dev::ServiceResponse<BoxBody>,
    Error = actix_web::Error,
> {
    let validator = Validator::new(
        idp.issuer(),
        vec![audience.into()],
        Duration::from_secs(300),
    )
    .await
    .unwrap();
    let exchanger = BasicExchanger::new(
        idp.issuer(),
        Some(audience.into()),
        None,
        Duration::from_secs(3600),
    )
    .await
    .unwrap();
    let auth_state = Arc::new(AuthState {
        mode: AuthMode::Enabled { validator, exchanger },
    });
    actix_web::test::init_service(
        App::new()
            .app_data(web::Data::from(auth_state.clone()))
            .service(
                web::scope("/v1")
                    .wrap(AuthMiddleware { state: auth_state.clone() })
                    .route(
                        "/auth/whoami",
                        web::get().to(kronos_api::handlers::auth::whoami),
                    )
                    .route(
                        "/auth/cache/flush",
                        web::post().to(kronos_api::handlers::auth::flush_cache),
                    ),
            ),
    )
    .await
}
