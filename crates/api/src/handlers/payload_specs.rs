use crate::extractors::AuthenticatedRequest;
use crate::router::AppState;
use actix_web::{web, HttpResponse};
use kronos_common::{
    db,
    error::AppError,
    models::payload_spec::{CreatePayloadSpec, UpdatePayloadSpec},
    pagination::{encode_cursor, PaginatedResponse, PaginationParams},
};

pub async fn create(
    state: web::Data<AppState>,
    _auth: AuthenticatedRequest,
    body: web::Json<CreatePayloadSpec>,
) -> Result<HttpResponse, AppError> {
    if !body.schema.is_object() {
        return Err(AppError::InvalidSchema(
            "Schema must be a JSON object".into(),
        ));
    }

    let spec = db::payload_specs::create(&state.pool, &body.name, &body.schema)
        .await
        .map_err(|e| match e {
            sqlx::Error::Database(ref db_err) if db_err.constraint().is_some() => {
                AppError::Conflict(format!("Payload spec '{}' already exists", body.name))
            }
            _ => AppError::from(e),
        })?;

    Ok(HttpResponse::Created().json(serde_json::json!({ "data": {
        "name": spec.name,
        "schema": spec.schema_json,
        "created_at": spec.created_at,
        "updated_at": spec.updated_at,
    }})))
}

pub async fn list(
    state: web::Data<AppState>,
    _auth: AuthenticatedRequest,
    params: web::Query<PaginationParams>,
) -> Result<HttpResponse, AppError> {
    let limit = params.effective_limit();
    let cursor = params.decode_cursor();
    let items = db::payload_specs::list(&state.pool, cursor.as_deref(), limit + 1).await?;

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
                "name": s.name, "schema": s.schema_json,
                "created_at": s.created_at, "updated_at": s.updated_at,
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
    path: web::Path<String>,
) -> Result<HttpResponse, AppError> {
    let name = path.into_inner();
    let spec = db::payload_specs::get(&state.pool, &name)
        .await?
        .ok_or_else(|| AppError::PayloadSpecNotFound(name))?;

    Ok(HttpResponse::Ok().json(serde_json::json!({ "data": {
        "name": spec.name, "schema": spec.schema_json,
        "created_at": spec.created_at, "updated_at": spec.updated_at,
    }})))
}

pub async fn update(
    state: web::Data<AppState>,
    _auth: AuthenticatedRequest,
    path: web::Path<String>,
    body: web::Json<UpdatePayloadSpec>,
) -> Result<HttpResponse, AppError> {
    let name = path.into_inner();
    if !body.schema.is_object() {
        return Err(AppError::InvalidSchema(
            "Schema must be a JSON object".into(),
        ));
    }

    let spec = db::payload_specs::update(&state.pool, &name, &body.schema)
        .await?
        .ok_or_else(|| AppError::PayloadSpecNotFound(name))?;

    Ok(HttpResponse::Ok().json(serde_json::json!({ "data": {
        "name": spec.name, "schema": spec.schema_json,
        "created_at": spec.created_at, "updated_at": spec.updated_at,
    }})))
}

pub async fn delete(
    state: web::Data<AppState>,
    _auth: AuthenticatedRequest,
    path: web::Path<String>,
) -> Result<HttpResponse, AppError> {
    let name = path.into_inner();
    if db::payload_specs::has_dependent_endpoints(&state.pool, &name).await? {
        return Err(AppError::Conflict(format!(
            "Payload spec '{}' has dependent endpoints",
            name
        )));
    }
    if !db::payload_specs::delete(&state.pool, &name).await? {
        return Err(AppError::PayloadSpecNotFound(name));
    }
    Ok(HttpResponse::NoContent().finish())
}
