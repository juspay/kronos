use actix_web::{dev::Payload, web, Error, FromRequest, HttpRequest, HttpResponse};
use std::future::{self, Ready};

use crate::router::AppState;

pub struct AuthenticatedRequest;

impl FromRequest for AuthenticatedRequest {
    type Error = Error;
    type Future = Ready<Result<Self, Self::Error>>;

    fn from_request(req: &HttpRequest, _payload: &mut Payload) -> Self::Future {
        let state = req.app_data::<web::Data<AppState>>();

        let auth_header = req
            .headers()
            .get("authorization")
            .and_then(|v| v.to_str().ok());

        let result = match (state, auth_header) {
            (Some(state), Some(header)) if header.starts_with("Bearer ") => {
                let token = &header[7..];
                if token == state.config.api_key {
                    Ok(AuthenticatedRequest)
                } else {
                    Err(actix_web::error::InternalError::from_response(
                        "Invalid API key",
                        HttpResponse::Unauthorized().json(serde_json::json!({
                            "error": { "code": "UNAUTHORIZED", "message": "Invalid API key" }
                        })),
                    )
                    .into())
                }
            }
            _ => Err(actix_web::error::InternalError::from_response(
                "Missing Authorization header",
                HttpResponse::Unauthorized().json(serde_json::json!({
                    "error": { "code": "UNAUTHORIZED", "message": "Missing Authorization header" }
                })),
            )
            .into()),
        };

        future::ready(result)
    }
}
