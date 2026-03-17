use actix_web::{web, HttpResponse};
use kronos_common::{db, error::AppError};
use crate::extractors::AuthenticatedRequest;
use crate::router::AppState;

pub async fn get(
    state: web::Data<AppState>,
    _auth: AuthenticatedRequest,
    path: web::Path<String>,
) -> Result<HttpResponse, AppError> {
    let execution_id = path.into_inner();
    let exec = db::executions::get(&state.pool, &execution_id).await?
        .ok_or_else(|| AppError::ExecutionNotFound(execution_id))?;

    Ok(HttpResponse::Ok().json(serde_json::json!({
        "execution_id": exec.execution_id,
        "job_id": exec.job_id,
        "endpoint": exec.endpoint,
        "endpoint_type": exec.endpoint_type,
        "status": exec.status,
        "input": exec.input,
        "output": exec.output,
        "attempt_count": exec.attempt_count,
        "max_attempts": exec.max_attempts,
        "worker_id": exec.worker_id,
        "run_at": exec.run_at,
        "started_at": exec.started_at,
        "completed_at": exec.completed_at,
        "duration_ms": exec.duration_ms,
        "created_at": exec.created_at,
    })))
}

pub async fn cancel(
    state: web::Data<AppState>,
    _auth: AuthenticatedRequest,
    path: web::Path<String>,
) -> Result<HttpResponse, AppError> {
    let execution_id = path.into_inner();
    let exec = db::executions::get(&state.pool, &execution_id).await?
        .ok_or_else(|| AppError::ExecutionNotFound(execution_id.clone()))?;

    match exec.status.as_str() {
        "PENDING" | "QUEUED" => {
            let cancelled = db::executions::cancel(&state.pool, &execution_id).await?
                .ok_or_else(|| AppError::ExecutionNotCancellable("Could not cancel".into()))?;
            Ok(HttpResponse::Ok().json(serde_json::json!({
                "execution_id": cancelled.execution_id,
                "status": cancelled.status,
            })))
        }
        _ => Err(AppError::ExecutionNotCancellable(format!(
            "Execution is in {} state", exec.status
        ))),
    }
}

pub async fn list_attempts(
    state: web::Data<AppState>,
    _auth: AuthenticatedRequest,
    path: web::Path<String>,
) -> Result<HttpResponse, AppError> {
    let execution_id = path.into_inner();
    let _ = db::executions::get(&state.pool, &execution_id).await?
        .ok_or_else(|| AppError::ExecutionNotFound(execution_id.clone()))?;

    let attempts = db::attempts::list_for_execution(&state.pool, &execution_id).await?;
    let items: Vec<serde_json::Value> = attempts.into_iter().map(|a| serde_json::json!({
        "attempt_id": a.attempt_id,
        "attempt_number": a.attempt_number,
        "status": a.status,
        "started_at": a.started_at,
        "completed_at": a.completed_at,
        "duration_ms": a.duration_ms,
        "output": a.output,
        "error": a.error,
    })).collect();

    Ok(HttpResponse::Ok().json(serde_json::json!({ "items": items })))
}

pub async fn list_logs(
    state: web::Data<AppState>,
    _auth: AuthenticatedRequest,
    path: web::Path<String>,
) -> Result<HttpResponse, AppError> {
    let execution_id = path.into_inner();
    let _ = db::executions::get(&state.pool, &execution_id).await?
        .ok_or_else(|| AppError::ExecutionNotFound(execution_id.clone()))?;

    let logs = db::execution_logs::list_for_execution(&state.pool, &execution_id).await?;
    let items: Vec<serde_json::Value> = logs.into_iter().map(|l| serde_json::json!({
        "log_id": l.log_id,
        "attempt_number": l.attempt_number,
        "level": l.level,
        "message": l.message,
        "logged_at": l.logged_at,
    })).collect();

    Ok(HttpResponse::Ok().json(serde_json::json!({ "items": items })))
}
