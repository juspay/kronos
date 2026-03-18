use actix_web::{dev::Payload, web, Error, FromRequest, HttpMessage, HttpRequest, HttpResponse};
use kronos_common::tenant::WorkspaceContext;
use std::future::{self, Future};
use std::pin::Pin;

use crate::router::AppState;

pub struct AuthenticatedRequest;

impl FromRequest for AuthenticatedRequest {
    type Error = Error;
    type Future = future::Ready<Result<Self, Self::Error>>;

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

/// Extracts workspace context from X-Org-Id and X-Workspace-Id headers,
/// resolves the schema_name from the database.
pub struct Workspace(pub WorkspaceContext);

impl FromRequest for Workspace {
    type Error = Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self, Self::Error>>>>;

    fn from_request(req: &HttpRequest, _payload: &mut Payload) -> Self::Future {
        let org_id = req
            .headers()
            .get("x-org-id")
            .and_then(|v| v.to_str().ok())
            .map(String::from);

        let workspace_id = req
            .headers()
            .get("x-workspace-id")
            .and_then(|v| v.to_str().ok())
            .map(String::from);

        let state = req.app_data::<web::Data<AppState>>().cloned();

        // Check if already resolved and stored in extensions
        if let Some(ctx) = req.extensions().get::<WorkspaceContext>().cloned() {
            return Box::pin(future::ready(Ok(Workspace(ctx))));
        }

        Box::pin(async move {
            let (org_id, workspace_id) = match (org_id, workspace_id) {
                (Some(o), Some(w)) => (o, w),
                _ => {
                    return Err(actix_web::error::InternalError::from_response(
                        "Missing workspace headers",
                        HttpResponse::BadRequest().json(serde_json::json!({
                            "error": {
                                "code": "MISSING_WORKSPACE",
                                "message": "X-Org-Id and X-Workspace-Id headers are required"
                            }
                        })),
                    )
                    .into())
                }
            };

            let state = state.ok_or_else(|| {
                actix_web::error::InternalError::from_response(
                    "Internal error",
                    HttpResponse::InternalServerError().finish(),
                )
            })?;

            let schema_name = kronos_common::db::workspaces::resolve_schema(
                &state.pool,
                &org_id,
                &workspace_id,
            )
            .await
            .map_err(|_| {
                actix_web::error::InternalError::from_response(
                    "Database error",
                    HttpResponse::InternalServerError().json(serde_json::json!({
                        "error": { "code": "INTERNAL_ERROR", "message": "Failed to resolve workspace" }
                    })),
                )
            })?
            .ok_or_else(|| {
                actix_web::error::InternalError::from_response(
                    "Workspace not found",
                    HttpResponse::NotFound().json(serde_json::json!({
                        "error": {
                            "code": "WORKSPACE_NOT_FOUND",
                            "message": format!("Workspace {} not found in org {}", workspace_id, org_id)
                        }
                    })),
                )
            })?;

            Ok(Workspace(WorkspaceContext {
                org_id,
                workspace_id,
                schema_name,
            }))
        })
    }
}
