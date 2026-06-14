//! Auth-related endpoints. Currently just `whoami`; `/v1/auth/cache/flush`
//! lands in the next task.

use actix_web::{HttpResponse, Responder};

use crate::extractors::AuthenticatedRequest;

/// `GET /v1/auth/whoami` — returns the current request's `Identity` as JSON.
///
/// The response shape matches `oidc_rs::Identity`'s `Serialize` impl:
/// `{"type":"bearer", "iss":"...", "sub":"...", "email":"...", "name":"...", "scopes":[...]}`
/// or `{"type":"basic", ...}` or `{"type":"disabled"}`.
pub async fn whoami(req: AuthenticatedRequest) -> impl Responder {
    HttpResponse::Ok().json(&req.0)
}
