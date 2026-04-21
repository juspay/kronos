use crate::extractors::{AuthenticatedRequest, Workspace};
use crate::router::AppState;
use actix_web::{web, HttpResponse};
use kronos_common::{
    crypto, db,
    error::AppError,
    models::secret::{CreateSecret, SecretResponse, UpdateSecret},
    pagination::{encode_cursor, PaginatedResponse, PaginationParams},
};

pub async fn create(
    state: web::Data<AppState>,
    _auth: AuthenticatedRequest,
    ws: Workspace,
    body: web::Json<CreateSecret>,
) -> Result<HttpResponse, AppError> {
    let encrypted = crypto::encrypt(&body.value, &state.config.crypto.encryption_key)
        .map_err(|e| AppError::Internal(format!("Encryption failed: {}", e)))?;

    let mut conn = kronos_common::db::scoped::scoped_connection(&state.pool, &ws.0.schema_name)
        .await
        .map_err(AppError::from)?;

    let secret = db::secrets::create(&mut *conn, &body.name, &encrypted)
        .await
        .map_err(|e| match e {
            sqlx::Error::Database(ref db_err) if db_err.constraint().is_some() => {
                AppError::Conflict(format!("Secret '{}' already exists", body.name))
            }
            _ => AppError::from(e),
        })?;

    let resp = SecretResponse::from(secret);
    Ok(HttpResponse::Created().json(serde_json::json!({ "data": {
        "name": resp.name, "created_at": resp.created_at, "updated_at": resp.updated_at,
    }})))
}

pub async fn list(
    state: web::Data<AppState>,
    _auth: AuthenticatedRequest,
    ws: Workspace,
    params: web::Query<PaginationParams>,
) -> Result<HttpResponse, AppError> {
    let mut conn = kronos_common::db::scoped::scoped_connection(&state.pool, &ws.0.schema_name)
        .await
        .map_err(AppError::from)?;
    let limit = params.effective_limit();
    let cursor = params.decode_cursor();
    let items = db::secrets::list(&mut *conn, cursor.as_deref(), limit + 1).await?;

    let has_more = items.len() as i64 > limit;
    let items: Vec<_> = items.into_iter().take(limit as usize).collect();
    let next_cursor = if has_more {
        items.last().map(|s| encode_cursor(&s.name))
    } else {
        None
    };

    let data: Vec<serde_json::Value> = items
        .into_iter()
        .map(|s| {
            serde_json::json!({
                "name": s.name, "created_at": s.created_at, "updated_at": s.updated_at,
            })
        })
        .collect();

    Ok(HttpResponse::Ok().json(PaginatedResponse {
        data,
        cursor: next_cursor,
    }))
}

pub async fn get(
    state: web::Data<AppState>,
    _auth: AuthenticatedRequest,
    ws: Workspace,
    path: web::Path<String>,
) -> Result<HttpResponse, AppError> {
    let mut conn = kronos_common::db::scoped::scoped_connection(&state.pool, &ws.0.schema_name)
        .await
        .map_err(AppError::from)?;
    let name = path.into_inner();
    let secret = db::secrets::get(&mut *conn, &name)
        .await?
        .ok_or_else(|| AppError::SecretNotFound(name))?;

    Ok(HttpResponse::Ok().json(serde_json::json!({ "data": {
        "name": secret.name, "created_at": secret.created_at, "updated_at": secret.updated_at,
    }})))
}

pub async fn update(
    state: web::Data<AppState>,
    _auth: AuthenticatedRequest,
    ws: Workspace,
    path: web::Path<String>,
    body: web::Json<UpdateSecret>,
) -> Result<HttpResponse, AppError> {
    let name = path.into_inner();
    let encrypted = crypto::encrypt(&body.value, &state.config.crypto.encryption_key)
        .map_err(|e| AppError::Internal(format!("Encryption failed: {}", e)))?;

    let mut conn = kronos_common::db::scoped::scoped_connection(&state.pool, &ws.0.schema_name)
        .await
        .map_err(AppError::from)?;

    let secret = db::secrets::update(&mut *conn, &name, &encrypted)
        .await?
        .ok_or_else(|| AppError::SecretNotFound(name))?;

    Ok(HttpResponse::Ok().json(serde_json::json!({ "data": {
        "name": secret.name, "created_at": secret.created_at, "updated_at": secret.updated_at,
    }})))
}

pub async fn delete(
    state: web::Data<AppState>,
    _auth: AuthenticatedRequest,
    ws: Workspace,
    path: web::Path<String>,
) -> Result<HttpResponse, AppError> {
    let mut conn = kronos_common::db::scoped::scoped_connection(&state.pool, &ws.0.schema_name)
        .await
        .map_err(AppError::from)?;
    let name = path.into_inner();
    if db::secrets::has_dependent_endpoints(&mut *conn, &name).await? {
        return Err(AppError::Conflict(format!(
            "Secret '{}' is referenced by endpoints",
            name
        )));
    }
    if !db::secrets::delete(&mut *conn, &name).await? {
        return Err(AppError::SecretNotFound(name));
    }
    Ok(HttpResponse::NoContent().finish())
}
