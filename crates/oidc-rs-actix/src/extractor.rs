//! Extractor that reads the [`Identity`] injected by [`crate::AuthMiddleware`].

use actix_web::{dev::Payload, Error, FromRequest, HttpMessage, HttpRequest, HttpResponse};
use oidc_rs::Identity;
use std::future::{self, Ready};

/// Pulls the [`Identity`] out of the request extensions. If the middleware
/// is not configured, returns HTTP 500 — that's a programming error.
pub struct Authenticated(pub Identity);

impl FromRequest for Authenticated {
    type Error = Error;
    type Future = Ready<Result<Self, Self::Error>>;

    fn from_request(req: &HttpRequest, _payload: &mut Payload) -> Self::Future {
        let identity = req.extensions().get::<Identity>().cloned();
        future::ready(match identity {
            Some(id) => Ok(Authenticated(id)),
            None => Err(actix_web::error::InternalError::from_response(
                "auth middleware not configured",
                HttpResponse::InternalServerError().json(serde_json::json!({
                    "error": {
                        "code": "INTERNAL_ERROR",
                        "message": "auth middleware not configured"
                    }
                })),
            )
            .into()),
        })
    }
}
