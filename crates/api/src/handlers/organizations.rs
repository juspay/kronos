use crate::extractors::AuthenticatedRequest;
use crate::router::AppState;
use actix_web::{web, HttpResponse};
use kronos_common::{
    db,
    error::AppError,
    models::organization::{CreateOrganization, UpdateOrganization},
};

pub async fn create(
    state: web::Data<AppState>,
    _auth: AuthenticatedRequest,
    body: web::Json<CreateOrganization>,
) -> Result<HttpResponse, AppError> {
    let org = db::organizations::create(&state.pool, &body.name, &body.slug)
        .await
        .map_err(|e| match e {
            sqlx::Error::Database(ref db_err) if db_err.constraint().is_some() => {
                AppError::Conflict(format!(
                    "Organization with slug '{}' already exists",
                    body.slug
                ))
            }
            _ => AppError::from(e),
        })?;

    Ok(HttpResponse::Created().json(serde_json::json!({ "data": {
        "org_id": org.org_id,
        "name": org.name,
        "slug": org.slug,
        "status": org.status,
        "created_at": org.created_at,
    }})))
}

pub async fn list(
    state: web::Data<AppState>,
    _auth: AuthenticatedRequest,
) -> Result<HttpResponse, AppError> {
    let orgs = db::organizations::list(&state.pool).await?;
    let data: Vec<serde_json::Value> = orgs
        .into_iter()
        .map(|o| {
            serde_json::json!({
                "org_id": o.org_id,
                "name": o.name,
                "slug": o.slug,
                "status": o.status,
                "created_at": o.created_at,
            })
        })
        .collect();

    Ok(HttpResponse::Ok().json(serde_json::json!({ "data": data })))
}

pub async fn get(
    state: web::Data<AppState>,
    _auth: AuthenticatedRequest,
    path: web::Path<String>,
) -> Result<HttpResponse, AppError> {
    let org_id = path.into_inner();
    let org = db::organizations::get(&state.pool, &org_id)
        .await?
        .ok_or_else(|| AppError::OrgNotFound(format!("Organization {} not found", org_id)))?;

    Ok(HttpResponse::Ok().json(serde_json::json!({ "data": {
        "org_id": org.org_id,
        "name": org.name,
        "slug": org.slug,
        "status": org.status,
        "created_at": org.created_at,
        "updated_at": org.updated_at,
    }})))
}

pub async fn update(
    state: web::Data<AppState>,
    _auth: AuthenticatedRequest,
    path: web::Path<String>,
    body: web::Json<UpdateOrganization>,
) -> Result<HttpResponse, AppError> {
    let org_id = path.into_inner();

    let name = body
        .name
        .as_deref()
        .ok_or_else(|| AppError::InvalidRequest("name is required".into()))?;

    let org = db::organizations::update(&state.pool, &org_id, name)
        .await?
        .ok_or_else(|| AppError::OrgNotFound(format!("Organization {} not found", org_id)))?;

    Ok(HttpResponse::Ok().json(serde_json::json!({ "data": {
        "org_id": org.org_id,
        "name": org.name,
        "slug": org.slug,
        "status": org.status,
        "created_at": org.created_at,
        "updated_at": org.updated_at,
    }})))
}
