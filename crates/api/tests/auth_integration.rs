use actix_web::test;
use base64::Engine;
use std::sync::atomic::Ordering;

mod common;

#[actix_web::test]
async fn whoami_in_disabled_mode_returns_disabled_identity() {
    let app = common::start_api_disabled().await;
    let req = test::TestRequest::get().uri("/v1/auth/whoami").to_request();
    let resp: serde_json::Value = test::call_and_read_body_json(&app, req).await;
    assert_eq!(resp, serde_json::json!({"type": "disabled"}));
}

#[actix_web::test]
async fn flush_cache_in_disabled_mode_is_a_noop() {
    let app = common::start_api_disabled().await;
    let req = test::TestRequest::post()
        .uri("/v1/auth/cache/flush")
        .set_json(&serde_json::json!({}))
        .to_request();
    let resp: serde_json::Value = test::call_and_read_body_json(&app, req).await;
    assert_eq!(resp["positive_evicted"], 0);
    assert_eq!(resp["negative_evicted"], 0);
}

#[actix_web::test]
async fn flush_cache_returns_400_on_malformed_json() {
    // Regression for the silent-flush footgun: a typo'd body must NOT cause
    // a full cache flush. Use disabled mode because we just want the body
    // parsing to be exercised — the JSON parse failure fires before the
    // exchanger lookup.
    let app = common::start_api_disabled().await;
    let req = test::TestRequest::post()
        .uri("/v1/auth/cache/flush")
        .insert_header(("content-type", "application/json"))
        .set_payload("{not valid json}")
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), 400);
    let body = test::read_body(resp).await;
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["error"]["code"], "MALFORMED_BODY");
}

#[actix_web::test]
async fn bearer_with_valid_jwt_returns_identity_bearer() {
    let idp = common::MockIdp::start().await;
    let app = common::start_api_enabled(&idp, "kronos-api").await;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let jwt = idp.mint(serde_json::json!({
        "iss": idp.issuer(),
        "sub": "alice",
        "aud": "kronos-api",
        "exp": now + 300,
        "iat": now,
        "email": "alice@example.com",
    }));
    let req = test::TestRequest::get()
        .uri("/v1/auth/whoami")
        .insert_header(("Authorization", format!("Bearer {jwt}")))
        .to_request();
    let resp: serde_json::Value = test::call_and_read_body_json(&app, req).await;
    assert_eq!(resp["type"], "bearer");
    assert_eq!(resp["sub"], "alice");
    assert_eq!(resp["email"], "alice@example.com");
}

#[actix_web::test]
async fn basic_with_valid_creds_exchanges_and_returns_identity_basic() {
    let idp = common::MockIdp::start().await;
    idp.install_token_endpoint("kronos-api").await;
    let app = common::start_api_enabled(&idp, "kronos-api").await;

    let header = format!(
        "Basic {}",
        base64::engine::general_purpose::STANDARD.encode("svc-1:s3cret")
    );
    let req = test::TestRequest::get()
        .uri("/v1/auth/whoami")
        .insert_header(("Authorization", header))
        .to_request();
    let resp: serde_json::Value = test::call_and_read_body_json(&app, req).await;
    assert_eq!(resp["type"], "basic");
    assert_eq!(resp["sub"], "test-m2m-client");
    assert_eq!(idp.token_calls.load(Ordering::SeqCst), 1);
}

#[actix_web::test]
async fn bearer_scheme_is_case_insensitive() {
    // RFC 7235 § 2.1: auth-scheme tokens are case-insensitive. Make sure
    // `bearer <jwt>` (lowercase) authenticates just like `Bearer <jwt>`.
    let idp = common::MockIdp::start().await;
    let app = common::start_api_enabled(&idp, "kronos-api").await;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let jwt = idp.mint(serde_json::json!({
        "iss": idp.issuer(),
        "sub": "alice",
        "aud": "kronos-api",
        "exp": now + 300,
        "iat": now,
    }));
    let req = test::TestRequest::get()
        .uri("/v1/auth/whoami")
        .insert_header(("Authorization", format!("bearer {jwt}")))
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), 200);
}

#[actix_web::test]
async fn missing_authorization_returns_401() {
    let idp = common::MockIdp::start().await;
    let app = common::start_api_enabled(&idp, "kronos-api").await;
    let req = test::TestRequest::get().uri("/v1/auth/whoami").to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), 401);
}

#[actix_web::test]
async fn expired_bearer_returns_401() {
    let idp = common::MockIdp::start().await;
    let app = common::start_api_enabled(&idp, "kronos-api").await;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let jwt = idp.mint(serde_json::json!({
        "iss": idp.issuer(),
        "sub": "alice",
        "aud": "kronos-api",
        "exp": now - 3600,
        "iat": now - 7200,
    }));
    let req = test::TestRequest::get()
        .uri("/v1/auth/whoami")
        .insert_header(("Authorization", format!("Bearer {jwt}")))
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), 401);
}
