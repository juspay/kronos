use crate::extractors::AuthenticatedRequest;
use crate::router::AppState;
use actix_web::{web, HttpResponse};
use kronos_common::{
    db,
    error::AppError,
    models::endpoint::{CreateEndpoint, EndpointType, UpdateEndpoint},
    pagination::{encode_cursor, PaginatedResponse, PaginationParams},
};

pub async fn create(
    state: web::Data<AppState>,
    _auth: AuthenticatedRequest,
    body: web::Json<CreateEndpoint>,
) -> Result<HttpResponse, AppError> {
    if EndpointType::from_str_val(&body.endpoint_type).is_none() {
        return Err(AppError::InvalidRequest(format!(
            "Invalid endpoint type: {}. Must be HTTP, KAFKA, or REDIS_STREAM",
            body.endpoint_type
        )));
    }

    if let Some(ref ps) = body.payload_spec {
        if db::payload_specs::get(&state.pool, ps).await?.is_none() {
            return Err(AppError::InvalidPayloadSpecRef(ps.clone()));
        }
    }
    if let Some(ref cfg) = body.config {
        if db::configs::get(&state.pool, cfg).await?.is_none() {
            return Err(AppError::InvalidConfigRef(cfg.clone()));
        }
    }

    let retry_json = body
        .retry_policy
        .as_ref()
        .map(|rp| serde_json::to_value(rp).unwrap());

    let ep = db::endpoints::create(
        &state.pool,
        &body.name,
        &body.endpoint_type,
        body.payload_spec.as_deref(),
        body.config.as_deref(),
        &body.spec,
        retry_json.as_ref(),
    )
    .await
    .map_err(|e| match e {
        sqlx::Error::Database(ref db_err) if db_err.constraint().is_some() => {
            AppError::Conflict(format!("Endpoint '{}' already exists", body.name))
        }
        _ => AppError::from(e),
    })?;

    Ok(HttpResponse::Created().json(serde_json::json!({ "data": endpoint_to_json(&ep) })))
}

pub async fn list(
    state: web::Data<AppState>,
    _auth: AuthenticatedRequest,
    params: web::Query<PaginationParams>,
) -> Result<HttpResponse, AppError> {
    let limit = params.effective_limit();
    let cursor = params.decode_cursor();
    let items = db::endpoints::list(&state.pool, cursor.as_deref(), limit + 1).await?;

    let has_more = items.len() as i64 > limit;
    let items: Vec<_> = items.into_iter().take(limit as usize).collect();
    let next_cursor = if has_more {
        items.last().map(|e| encode_cursor(&e.name))
    } else {
        None
    };
    let data: Vec<serde_json::Value> = items.into_iter().map(|e| endpoint_to_json(&e)).collect();

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
    let ep = db::endpoints::get(&state.pool, &name)
        .await?
        .ok_or_else(|| AppError::EndpointNotFound(name))?;
    Ok(HttpResponse::Ok().json(serde_json::json!({ "data": endpoint_to_json(&ep) })))
}

pub async fn update(
    state: web::Data<AppState>,
    _auth: AuthenticatedRequest,
    path: web::Path<String>,
    body: web::Json<UpdateEndpoint>,
) -> Result<HttpResponse, AppError> {
    let name = path.into_inner();
    if let Some(ref ps) = body.payload_spec {
        if db::payload_specs::get(&state.pool, ps).await?.is_none() {
            return Err(AppError::InvalidPayloadSpecRef(ps.clone()));
        }
    }
    if let Some(ref cfg) = body.config {
        if db::configs::get(&state.pool, cfg).await?.is_none() {
            return Err(AppError::InvalidConfigRef(cfg.clone()));
        }
    }

    let retry_json = body
        .retry_policy
        .as_ref()
        .map(|rp| serde_json::to_value(rp).unwrap());

    let ep = db::endpoints::update(
        &state.pool,
        &name,
        body.spec.as_ref(),
        body.config.as_deref(),
        body.payload_spec.as_deref(),
        retry_json.as_ref(),
    )
    .await?
    .ok_or_else(|| AppError::EndpointNotFound(name))?;

    Ok(HttpResponse::Ok().json(serde_json::json!({ "data": endpoint_to_json(&ep) })))
}

pub async fn delete(
    state: web::Data<AppState>,
    _auth: AuthenticatedRequest,
    path: web::Path<String>,
) -> Result<HttpResponse, AppError> {
    let name = path.into_inner();
    if db::endpoints::has_active_jobs(&state.pool, &name).await? {
        return Err(AppError::Conflict(format!(
            "Endpoint '{}' has active jobs",
            name
        )));
    }
    if !db::endpoints::delete(&state.pool, &name).await? {
        return Err(AppError::EndpointNotFound(name));
    }
    Ok(HttpResponse::NoContent().finish())
}

fn endpoint_to_json(ep: &kronos_common::models::Endpoint) -> serde_json::Value {
    serde_json::json!({
        "name": ep.name,
        "type": ep.endpoint_type,
        "payload_spec": ep.payload_spec_ref,
        "config": ep.config_ref,
        "spec": ep.spec,
        "retry_policy": ep.retry_policy,
        "created_at": ep.created_at,
        "updated_at": ep.updated_at,
    })
}
