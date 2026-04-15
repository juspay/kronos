use crate::extractors::{AuthenticatedRequest, Workspace};
use crate::router::AppState;
use actix_web::{web, HttpResponse};
use kronos_common::{
    db,
    error::AppError,
    kms::KmsProviderType,
    models::secret::{CreateSecret, SecretResponse, UpdateSecret},
    pagination::{encode_cursor, PaginatedResponse, PaginationParams},
};

pub async fn create(
    state: web::Data<AppState>,
    _auth: AuthenticatedRequest,
    ws: Workspace,
    body: web::Json<CreateSecret>,
) -> Result<HttpResponse, AppError> {
    // Validate provider type
    KmsProviderType::from_str_val(&body.provider).ok_or_else(|| {
        AppError::InvalidRequest(format!(
            "Unsupported KMS provider: '{}'. Supported: aws, gcp, vault",
            body.provider
        ))
    })?;

    if body.reference.is_empty() {
        return Err(AppError::InvalidRequest(
            "Secret reference cannot be empty".into(),
        ));
    }

    let mut conn = kronos_common::db::scoped::scoped_connection(&state.pool, &ws.0.schema_name)
        .await
        .map_err(AppError::from)?;

    let secret = db::secrets::create(&mut *conn, &body.name, &body.provider, &body.reference)
        .await
        .map_err(|e| match e {
            sqlx::Error::Database(ref db_err) if db_err.constraint().is_some() => {
                AppError::Conflict(format!("Secret '{}' already exists", body.name))
            }
            _ => AppError::from(e),
        })?;

    let resp = SecretResponse::from(secret);
    Ok(HttpResponse::Created().json(serde_json::json!({ "data": resp })))
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

    let data: Vec<SecretResponse> = items.into_iter().map(SecretResponse::from).collect();

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

    let resp = SecretResponse::from(secret);
    Ok(HttpResponse::Ok().json(serde_json::json!({ "data": resp })))
}

pub async fn update(
    state: web::Data<AppState>,
    _auth: AuthenticatedRequest,
    ws: Workspace,
    path: web::Path<String>,
    body: web::Json<UpdateSecret>,
) -> Result<HttpResponse, AppError> {
    if let Some(ref provider) = body.provider {
        KmsProviderType::from_str_val(provider).ok_or_else(|| {
            AppError::InvalidRequest(format!(
                "Unsupported KMS provider: '{}'. Supported: aws, gcp, vault",
                provider
            ))
        })?;
    }

    if let Some(ref reference) = body.reference {
        if reference.is_empty() {
            return Err(AppError::InvalidRequest(
                "Secret reference cannot be empty".into(),
            ));
        }
    }

    let name = path.into_inner();
    let mut conn = kronos_common::db::scoped::scoped_connection(&state.pool, &ws.0.schema_name)
        .await
        .map_err(AppError::from)?;

    let secret = db::secrets::update(
        &mut *conn,
        &name,
        body.provider.as_deref(),
        body.reference.as_deref(),
    )
    .await?
    .ok_or_else(|| AppError::SecretNotFound(name))?;

    let resp = SecretResponse::from(secret);
    Ok(HttpResponse::Ok().json(serde_json::json!({ "data": resp })))
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
