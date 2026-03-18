use crate::extractors::{AuthenticatedRequest, Workspace};
use crate::router::AppState;
use actix_web::{web, HttpResponse};
use kronos_common::{
    db,
    error::AppError,
    models::config::{CreateConfig, UpdateConfig},
    pagination::{encode_cursor, PaginatedResponse, PaginationParams},
};

pub async fn create(
    state: web::Data<AppState>,
    _auth: AuthenticatedRequest,
    ws: Workspace,
    body: web::Json<CreateConfig>,
) -> Result<HttpResponse, AppError> {
    if !body.values.is_object() {
        return Err(AppError::InvalidRequest(
            "Values must be a JSON object".into(),
        ));
    }

    let mut conn = kronos_common::db::scoped::scoped_connection(&state.pool, &ws.0.schema_name)
        .await
        .map_err(AppError::from)?;

    let config = db::configs::create(&mut *conn, &body.name, &body.values)
        .await
        .map_err(|e| match e {
            sqlx::Error::Database(ref db_err) if db_err.constraint().is_some() => {
                AppError::Conflict(format!("Config '{}' already exists", body.name))
            }
            _ => AppError::from(e),
        })?;

    Ok(HttpResponse::Created().json(serde_json::json!({ "data": {
        "name": config.name, "values": config.values_json,
        "created_at": config.created_at, "updated_at": config.updated_at,
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
    let items = db::configs::list(&mut *conn, cursor.as_deref(), limit + 1).await?;

    let has_more = items.len() as i64 > limit;
    let items: Vec<_> = items.into_iter().take(limit as usize).collect();
    let next_cursor = if has_more {
        items.last().map(|c| encode_cursor(&c.name))
    } else {
        None
    };

    let data: Vec<serde_json::Value> = items
        .into_iter()
        .map(|c| {
            serde_json::json!({
                "name": c.name, "values": c.values_json,
                "created_at": c.created_at, "updated_at": c.updated_at,
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
    let config = db::configs::get(&mut *conn, &name)
        .await?
        .ok_or_else(|| AppError::ConfigNotFound(name))?;

    Ok(HttpResponse::Ok().json(serde_json::json!({ "data": {
        "name": config.name, "values": config.values_json,
        "created_at": config.created_at, "updated_at": config.updated_at,
    }})))
}

pub async fn update(
    state: web::Data<AppState>,
    _auth: AuthenticatedRequest,
    ws: Workspace,
    path: web::Path<String>,
    body: web::Json<UpdateConfig>,
) -> Result<HttpResponse, AppError> {
    if !body.values.is_object() {
        return Err(AppError::InvalidRequest(
            "Values must be a JSON object".into(),
        ));
    }

    let mut conn = kronos_common::db::scoped::scoped_connection(&state.pool, &ws.0.schema_name)
        .await
        .map_err(AppError::from)?;
    let name = path.into_inner();

    let config = db::configs::update(&mut *conn, &name, &body.values)
        .await?
        .ok_or_else(|| AppError::ConfigNotFound(name))?;

    Ok(HttpResponse::Ok().json(serde_json::json!({ "data": {
        "name": config.name, "values": config.values_json,
        "created_at": config.created_at, "updated_at": config.updated_at,
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
    if db::configs::has_dependent_endpoints(&mut *conn, &name).await? {
        return Err(AppError::Conflict(format!(
            "Config '{}' has dependent endpoints",
            name
        )));
    }
    if !db::configs::delete(&mut *conn, &name).await? {
        return Err(AppError::ConfigNotFound(name));
    }
    Ok(HttpResponse::NoContent().finish())
}
