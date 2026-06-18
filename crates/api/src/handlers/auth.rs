//! Auth-related endpoints: `whoami` and `flush_cache`.

use actix_web::{web, HttpResponse, Responder};
use serde::Deserialize;

use crate::extractors::AuthenticatedRequest;
use crate::middleware::AuthState;

/// `GET /v1/auth/whoami` — returns the current request's `Identity` as JSON.
///
/// The response shape matches `oidc_rs::Identity`'s `Serialize` impl:
/// `{"type":"bearer", "iss":"...", "sub":"...", "email":"...", "name":"...", "scopes":[...]}`
/// or `{"type":"basic", ...}` or `{"type":"disabled"}`.
pub async fn whoami(req: AuthenticatedRequest) -> impl Responder {
    HttpResponse::Ok().json(&req.0)
}

/// Body for `POST /v1/auth/cache/flush`. Empty body (or `{}`) flushes all
/// entries; `{"client_id": "..."}` flushes only that client's positive and
/// negative cache entries.
#[derive(Deserialize, Default)]
pub struct FlushRequest {
    #[serde(default)]
    client_id: Option<String>,
}

/// `POST /v1/auth/cache/flush` — evicts entries from the Basic→JWT exchange
/// cache. Use after rotating a client secret in the IdP so Kronos picks up
/// the new credential immediately instead of waiting for the cache TTL.
///
/// Response: `{"positive_evicted": N, "negative_evicted": M}`.
///
/// In auth-disabled mode there's no exchanger; the endpoint returns
/// `{"positive_evicted": 0, "negative_evicted": 0, "note": "..."}`.
pub async fn flush_cache(
    _req: AuthenticatedRequest,
    body: web::Bytes,
    state: web::Data<AuthState>,
) -> impl Responder {
    // Parse the body manually rather than `Option<web::Json<…>>`: the latter
    // returns `None` on malformed JSON, which would silently flush the entire
    // cache for a typo'd body. We want a 400 with a structured error instead.
    // An empty body is still accepted as `{}` for ergonomics.
    let req = if body.is_empty() {
        FlushRequest::default()
    } else {
        match serde_json::from_slice::<FlushRequest>(&body) {
            Ok(v) => v,
            Err(e) => {
                return HttpResponse::BadRequest().json(serde_json::json!({
                    "error": {
                        "code": "MALFORMED_BODY",
                        "message": format!("flush_cache body must be JSON: {e}"),
                    }
                }));
            }
        }
    };
    let Some(exchanger) = state.exchanger() else {
        return HttpResponse::Ok().json(serde_json::json!({
            "positive_evicted": 0,
            "negative_evicted": 0,
            "note": "auth mode is disabled; nothing to flush"
        }));
    };
    let (pos, neg) = exchanger.flush(req.client_id.as_deref());
    HttpResponse::Ok().json(serde_json::json!({
        "positive_evicted": pos,
        "negative_evicted": neg,
    }))
}
