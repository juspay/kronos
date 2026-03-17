use actix_web::{web, HttpResponse};
use kronos_common::{
    db, error::AppError,
    models::config::{CreateConfig, UpdateConfig},
    pagination::{encode_cursor, PaginatedResponse, PaginationParams},
};
use crate::extractors::AuthenticatedRequest;
use crate::router::AppState;

pub async fn create(
    state: web::Data<AppState>,
    _auth: AuthenticatedRequest,
    body: web::Json<CreateConfig>,
) -> Result<HttpResponse, AppError> {
    if !body.values.is_object() {
        return Err(AppError::InvalidRequest("Values must be a JSON object".into()));
    }

    let config = db::configs::create(&state.pool, &body.name, &body.values)
        .await
        .map_err(|e| match e {
            sqlx::Error::Database(ref db_err) if db_err.constraint().is_some() => {
                AppError::Conflict(format!("Config '{}' already exists", body.name))
            }
            _ => AppError::from(e),
        })?;

    Ok(HttpResponse::Created().json(serde_json::json!({
        "name": config.name, "values": config.values_json,
        "created_at": config.created_at, "updated_at": config.updated_at,
    })))
}

pub async fn list(
    state: web::Data<AppState>,
    _auth: AuthenticatedRequest,
    params: web::Query<PaginationParams>,
) -> Result<HttpResponse, AppError> {
    let limit = params.effective_limit();
    let cursor = params.decode_cursor();
    let items = db::configs::list(&state.pool, cursor.as_deref(), limit + 1).await?;

    let has_more = items.len() as i64 > limit;
    let items: Vec<_> = items.into_iter().take(limit as usize).collect();
    let next_cursor = if has_more { items.last().map(|c| encode_cursor(&c.name)) } else { None };

    let items: Vec<serde_json::Value> = items.into_iter().map(|c| serde_json::json!({
        "name": c.name, "values": c.values_json,
        "created_at": c.created_at, "updated_at": c.updated_at,
    })).collect();

    Ok(HttpResponse::Ok().json(PaginatedResponse { items, cursor: next_cursor }))
}

pub async fn get(
    state: web::Data<AppState>,
    _auth: AuthenticatedRequest,
    path: web::Path<String>,
) -> Result<HttpResponse, AppError> {
    let name = path.into_inner();
    let config = db::configs::get(&state.pool, &name).await?
        .ok_or_else(|| AppError::ConfigNotFound(name))?;

    Ok(HttpResponse::Ok().json(serde_json::json!({
        "name": config.name, "values": config.values_json,
        "created_at": config.created_at, "updated_at": config.updated_at,
    })))
}

pub async fn update(
    state: web::Data<AppState>,
    _auth: AuthenticatedRequest,
    path: web::Path<String>,
    body: web::Json<UpdateConfig>,
) -> Result<HttpResponse, AppError> {
    let name = path.into_inner();
    if !body.values.is_object() {
        return Err(AppError::InvalidRequest("Values must be a JSON object".into()));
    }

    let config = db::configs::update(&state.pool, &name, &body.values).await?
        .ok_or_else(|| AppError::ConfigNotFound(name))?;

    Ok(HttpResponse::Ok().json(serde_json::json!({
        "name": config.name, "values": config.values_json,
        "created_at": config.created_at, "updated_at": config.updated_at,
    })))
}

pub async fn delete(
    state: web::Data<AppState>,
    _auth: AuthenticatedRequest,
    path: web::Path<String>,
) -> Result<HttpResponse, AppError> {
    let name = path.into_inner();
    if db::configs::has_dependent_endpoints(&state.pool, &name).await? {
        return Err(AppError::Conflict(format!("Config '{}' has dependent endpoints", name)));
    }
    if !db::configs::delete(&state.pool, &name).await? {
        return Err(AppError::ConfigNotFound(name));
    }
    Ok(HttpResponse::NoContent().finish())
}
