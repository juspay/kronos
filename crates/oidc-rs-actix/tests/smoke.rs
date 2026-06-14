//! Smoke test: the middleware in disabled mode injects Identity::Disabled.

use actix_web::{test, web, App, HttpResponse};
use oidc_rs_actix::{AuthMiddleware, AuthMode, AuthState, Authenticated};
use std::sync::Arc;

#[actix_web::test]
async fn disabled_mode_injects_disabled_identity() {
    async fn handler(a: Authenticated) -> HttpResponse {
        HttpResponse::Ok().json(&a.0)
    }

    let state = Arc::new(AuthState { mode: AuthMode::Disabled });
    let app = test::init_service(
        App::new().service(
            web::scope("")
                .wrap(AuthMiddleware { state })
                .route("/whoami", web::get().to(handler)),
        ),
    )
    .await;

    let req = test::TestRequest::get().uri("/whoami").to_request();
    let resp: serde_json::Value = test::call_and_read_body_json(&app, req).await;
    assert_eq!(resp, serde_json::json!({"type": "disabled"}));
}
