use crate::extractors::AuthenticatedRequest;
use crate::router::AppState;
use actix_web::{web, HttpResponse};
use kronos_common::{
    db,
    error::AppError,
    models::workspace::CreateWorkspace,
    tenant::{build_schema_name, validate_slug},
};

pub async fn create(
    state: web::Data<AppState>,
    _auth: AuthenticatedRequest,
    path: web::Path<String>,
    body: web::Json<CreateWorkspace>,
) -> Result<HttpResponse, AppError> {
    let org_ref = path.into_inner();

    if !validate_slug(&body.slug) {
        return Err(AppError::InvalidRequest(format!(
            "Invalid slug '{}': must be 1-25 chars of lowercase letters, digits, and \
             interior hyphens (no leading or trailing hyphen)",
            body.slug
        )));
    }

    // Resolve the org (by id or slug). Use its canonical org_id for the stored
    // row and schema name so the workspace is keyed consistently no matter how
    // the caller addressed the org.
    let org = db::organizations::get(&state.pool, &org_ref)
        .await?
        .ok_or_else(|| AppError::OrgNotFound(format!("Organization {} not found", org_ref)))?;

    let schema_name = build_schema_name(&org.org_id, &body.slug);

    let workspace = db::workspaces::create(
        &state.pool,
        &org.org_id,
        &body.name,
        &body.slug,
        &schema_name,
        &state.config.reaper.cron_expression,
    )
    .await
    .map_err(|e| match e {
        sqlx::Error::Database(ref db_err) if db_err.constraint().is_some() => AppError::Conflict(
            format!("Workspace with slug '{}' already exists in this org", body.slug),
        ),
        _ => AppError::from(e),
    })?;

    Ok(HttpResponse::Created().json(serde_json::json!({ "data": {
        "workspace_id": workspace.workspace_id,
        "org_id": workspace.org_id,
        "name": workspace.name,
        "slug": workspace.slug,
        "schema_name": workspace.schema_name,
        "status": workspace.status,
        "schema_version": workspace.schema_version,
        "created_at": workspace.created_at,
    }})))
}

pub async fn list(
    state: web::Data<AppState>,
    _auth: AuthenticatedRequest,
    path: web::Path<String>,
) -> Result<HttpResponse, AppError> {
    let org_id = path.into_inner();

    // Verify the org exists
    let _ = db::organizations::get(&state.pool, &org_id)
        .await?
        .ok_or_else(|| AppError::OrgNotFound(format!("Organization {} not found", org_id)))?;

    let workspaces = db::workspaces::list_for_org(&state.pool, &org_id).await?;
    let data: Vec<serde_json::Value> = workspaces
        .into_iter()
        .map(|w| {
            serde_json::json!({
                "workspace_id": w.workspace_id,
                "org_id": w.org_id,
                "name": w.name,
                "slug": w.slug,
                "schema_name": w.schema_name,
                "status": w.status,
                "schema_version": w.schema_version,
                "created_at": w.created_at,
            })
        })
        .collect();

    Ok(HttpResponse::Ok().json(serde_json::json!({ "data": data })))
}

pub async fn get(
    state: web::Data<AppState>,
    _auth: AuthenticatedRequest,
    path: web::Path<(String, String)>,
) -> Result<HttpResponse, AppError> {
    let (org_id, workspace_id) = path.into_inner();

    let workspace = db::workspaces::get_by_org_and_id(&state.pool, &org_id, &workspace_id)
        .await?
        .ok_or_else(|| {
            AppError::WorkspaceNotFound(format!(
                "Workspace {} not found in org {}",
                workspace_id, org_id
            ))
        })?;

    Ok(HttpResponse::Ok().json(serde_json::json!({ "data": {
        "workspace_id": workspace.workspace_id,
        "org_id": workspace.org_id,
        "name": workspace.name,
        "slug": workspace.slug,
        "schema_name": workspace.schema_name,
        "status": workspace.status,
        "schema_version": workspace.schema_version,
        "created_at": workspace.created_at,
        "updated_at": workspace.updated_at,
    }})))
}
