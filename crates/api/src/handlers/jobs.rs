use crate::extractors::{AuthenticatedRequest, JobFilters, Workspace};
use crate::router::AppState;
use actix_web::{web, HttpResponse};
use chrono::Utc;
use kronos_common::metrics as m;
use kronos_common::{
    db,
    db::DbContext,
    error::AppError,
    models::endpoint::EndpointType,
    models::job::{CreateJob, TriggerType, UpdateJob},
    models::pg_cron_expr::PgCronExpr,
    pagination::{encode_cursor, PaginatedResponse, PaginationParams},
    service,
    service::jobs::TriggerSpec,
    service::WorkspaceRef,
};
use uuid::Uuid;

pub async fn create(
    state: web::Data<AppState>,
    _auth: AuthenticatedRequest,
    ws: Workspace,
    body: web::Json<CreateJob>,
) -> Result<HttpResponse, AppError> {
    let trigger = TriggerType::from_str_val(&body.trigger)
        .ok_or_else(|| AppError::InvalidRequest(format!("Invalid trigger: {}", body.trigger)))?;

    // REST's own contract, applied before handing off to the shared core: a
    // keyless IMMEDIATE job gets a generated key so it is addressable, and a
    // DELAYED job must carry one (`require_idempotency_key`). Everything else —
    // guards, validation, transactions, pg_cron — belongs to the service.
    let generated_key;
    let idempotency_key = match (&trigger, body.idempotency_key.as_deref()) {
        (TriggerType::IMMEDIATE, None) => {
            generated_key = Uuid::new_v4().to_string();
            Some(generated_key.as_str())
        }
        (_, key) => key,
    };

    let trigger_spec = match trigger {
        TriggerType::IMMEDIATE => TriggerSpec::Immediate,
        TriggerType::DELAYED => TriggerSpec::Delayed { run_at: body.run_at },
        TriggerType::CRON => TriggerSpec::Cron {
            expression: body.cron.clone(),
            timezone: body.timezone.clone(),
            starts_at: body.starts_at,
            ends_at: body.ends_at,
            // REST computes the first run from the schedule.
            next_run_at: None,
        },
    };

    let outcome = service::jobs::create_job(
        WorkspaceRef::new(&state.pool, &ws.0.schema_name, state.prefix()),
        service::jobs::CreateJobRequest {
            endpoint: &body.endpoint,
            trigger: trigger_spec,
            input: body.input.as_ref(),
            idempotency_key,
            max_attempts: body.max_attempts,
            require_idempotency_key: matches!(trigger, TriggerType::DELAYED),
        },
    )
    .await?;

    let (job, execution) = match outcome {
        // Idempotent replay: the key was already used, nothing was written.
        service::jobs::CreateOutcome::AlreadyExists { job, execution } => {
            return Ok(HttpResponse::Ok()
                .json(serde_json::json!({ "data": job_response(&job, execution.as_deref()) })));
        }
        service::jobs::CreateOutcome::Created { job, execution } => (job, execution),
    };

    metrics::counter!(m::JOBS_CREATED_TOTAL,
        "trigger_type" => job.trigger_type.clone(),
        "endpoint" => body.endpoint.clone(),
        "schema" => ws.0.schema_name.clone(),
    )
    .increment(1);

    // Response shapes are per-trigger and predate the service layer; kept
    // field-for-field so the wire format does not move.
    let data = match trigger {
        TriggerType::IMMEDIATE => {
            let execution = execution.expect("IMMEDIATE jobs always create an execution");
            serde_json::json!({
                "job_id": job.job_id,
                "endpoint": job.endpoint,
                "endpoint_type": job.endpoint_type,
                "trigger": job.trigger_type,
                "status": job.status,
                "version": job.version,
                "idempotency_key": job.idempotency_key,
                "input": job.input,
                "execution": {
                    "execution_id": execution.execution_id,
                    "status": execution.status,
                    "created_at": execution.created_at,
                },
                "created_at": job.created_at,
            })
        }
        TriggerType::DELAYED => {
            let execution = execution.expect("DELAYED jobs always create an execution");
            serde_json::json!({
                "job_id": job.job_id,
                "endpoint": job.endpoint,
                "endpoint_type": job.endpoint_type,
                "trigger": job.trigger_type,
                "status": job.status,
                "version": job.version,
                "idempotency_key": job.idempotency_key,
                "input": job.input,
                "run_at": job.run_at,
                "execution": {
                    "execution_id": execution.execution_id,
                    "status": execution.status,
                    "created_at": execution.created_at,
                },
                "created_at": job.created_at,
            })
        }
        TriggerType::CRON => serde_json::json!({
            "job_id": job.job_id,
            "endpoint": job.endpoint,
            "endpoint_type": job.endpoint_type,
            "trigger": job.trigger_type,
            "status": job.status,
            "version": job.version,
            "cron": job.cron_expression,
            "timezone": job.cron_timezone,
            "starts_at": job.cron_starts_at,
            "ends_at": job.cron_ends_at,
            "next_run_at": job.cron_next_run_at,
            "input": job.input,
            "created_at": job.created_at,
        }),
    };

    Ok(HttpResponse::Created().json(serde_json::json!({ "data": data })))
}

pub async fn list(
    state: web::Data<AppState>,
    _auth: AuthenticatedRequest,
    ws: Workspace,
    params: web::Query<PaginationParams>,
    filters: JobFilters,
) -> Result<HttpResponse, AppError> {
    let prefix = state.prefix();
    let mut conn = kronos_common::db::scoped::scoped_connection(&state.pool, &ws.0.schema_name)
        .await
        .map_err(AppError::from)?;
    let mut db = DbContext::new(&mut *conn, prefix);
    let limit = params.effective_limit();
    let cursor = params.decode_cursor();
    let items = db::jobs::list(&mut db, cursor.as_deref(), limit + 1, &filters.0).await?;

    let has_more = items.len() as i64 > limit;
    let items: Vec<_> = items.into_iter().take(limit as usize).collect();
    let next_cursor = if has_more {
        items.last().map(|j| encode_cursor(&j.job_id))
    } else {
        None
    };
    let data: Vec<serde_json::Value> = items.into_iter().map(|j| job_summary(&j)).collect();

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
    let prefix = state.prefix();
    let mut conn = kronos_common::db::scoped::scoped_connection(&state.pool, &ws.0.schema_name)
        .await
        .map_err(AppError::from)?;
    let mut db = DbContext::new(&mut *conn, prefix);
    let job_id = path.into_inner();
    let job = db::jobs::get(&mut db, &job_id)
        .await?
        .ok_or_else(|| AppError::JobNotFound(job_id))?;
    let exec = db::executions::get_for_job(&mut db, &job.job_id).await?;
    Ok(HttpResponse::Ok().json(serde_json::json!({ "data": job_response(&job, exec.as_ref()) })))
}

pub async fn update(
    state: web::Data<AppState>,
    _auth: AuthenticatedRequest,
    ws: Workspace,
    path: web::Path<String>,
    body: web::Json<UpdateJob>,
) -> Result<HttpResponse, AppError> {
    let prefix = state.prefix();
    let mut conn = kronos_common::db::scoped::scoped_connection(&state.pool, &ws.0.schema_name)
        .await
        .map_err(AppError::from)?;
    let job_id = path.into_inner();
    let old_job = {
        let mut db = DbContext::new(&mut *conn, prefix);
        db::jobs::get(&mut db, &job_id)
            .await?
            .ok_or_else(|| AppError::JobNotFound(job_id.clone()))?
    };

    if old_job.trigger_type != "CRON" {
        return Err(AppError::JobNotUpdatable(
            "Only CRON jobs can be updated".into(),
        ));
    }
    if old_job.status != "ACTIVE" {
        return Err(AppError::JobNotUpdatable("Job is not active".into()));
    }
    // INTERNAL jobs are kronos-managed (today: the dogfooded reaper). Changing
    // the schedule or pushing ends_at into the past would break monitoring for
    // this workspace, with nothing left to re-provision the job afterwards.
    if EndpointType::from_str_val(&old_job.endpoint_type) == Some(EndpointType::INTERNAL) {
        return Err(AppError::JobNotUpdatable(
            "Internal kronos jobs cannot be modified through the API".into(),
        ));
    }

    let cron_expr =
        match body.cron.clone() {
            Some(c) => c,
            None => PgCronExpr::try_from(old_job.cron_expression.clone().ok_or_else(|| {
                AppError::InvalidCron("Existing job has no cron expression".into())
            })?)?,
        };
    let tz_str = body
        .timezone
        .as_deref()
        .unwrap_or(old_job.cron_timezone.as_deref().unwrap_or("UTC"));

    let schedule = cron_expr.to_schedule();
    let tz: chrono_tz::Tz = tz_str
        .parse()
        .map_err(|_| AppError::InvalidRequest(format!("Invalid timezone: {}", tz_str)))?;

    let next_run = service::jobs::compute_next_cron(&schedule, &tz, Utc::now())
        .ok_or_else(|| AppError::InvalidCron("No upcoming run".into()))?;

    let mut new_job = old_job.clone();
    new_job.cron_expression = Some(cron_expr.as_str().to_string());
    new_job.cron_timezone = Some(tz_str.to_string());
    new_job.cron_next_run_at = Some(next_run);
    new_job.version = old_job.version + 1;
    if let Some(ref input) = body.input {
        new_job.input = Some(input.clone());
    }
    if let Some(starts_at) = body.starts_at {
        new_job.cron_starts_at = Some(starts_at);
    }
    new_job.cron_ends_at = body.ends_at.or(old_job.cron_ends_at);

    // Drop the scoped connection before starting a transaction
    drop(conn);

    let mut tx = kronos_common::db::scoped::scoped_transaction(&state.pool, &ws.0.schema_name)
        .await
        .map_err(AppError::from)?;
    let mut db = DbContext::new(&mut *tx, prefix);

    let created = db::jobs::retire_and_replace(&mut db, &job_id, &new_job).await?;

    drop(db);

    // Unschedule the old pg_cron job and register the new one on the same tx as
    // the version flip, so the retire/replace and the schedule swap commit (or
    // roll back) atomically.
    db::jobs::unregister_pg_cron_conn(&mut *tx, &ws.0.schema_name, &job_id).await?;
    db::jobs::register_pg_cron_conn(
        &mut *tx,
        prefix,
        &ws.0.schema_name,
        &created.job_id,
        cron_expr.as_str(),
    )
    .await?;

    tx.commit().await.map_err(AppError::from)?;

    Ok(HttpResponse::Ok().json(serde_json::json!({ "data": {
        "job_id": created.job_id,
        "endpoint": created.endpoint,
        "endpoint_type": created.endpoint_type,
        "trigger": created.trigger_type,
        "status": created.status,
        "version": created.version,
        "previous_version_id": created.previous_version_id,
        "cron": created.cron_expression,
        "timezone": created.cron_timezone,
        "next_run_at": created.cron_next_run_at,
        "input": created.input,
        "created_at": created.created_at,
    }})))
}

pub async fn cancel(
    state: web::Data<AppState>,
    _auth: AuthenticatedRequest,
    ws: Workspace,
    path: web::Path<String>,
) -> Result<HttpResponse, AppError> {
    let cancelled = service::jobs::cancel_job(
        WorkspaceRef::new(&state.pool, &ws.0.schema_name, state.prefix()),
        &path.into_inner(),
    )
    .await?;

    Ok(HttpResponse::Ok().json(serde_json::json!({ "data": job_summary(&cancelled) })))
}

pub async fn status(
    state: web::Data<AppState>,
    _auth: AuthenticatedRequest,
    ws: Workspace,
    path: web::Path<String>,
) -> Result<HttpResponse, AppError> {
    let prefix = state.prefix();
    let mut conn = kronos_common::db::scoped::scoped_connection(&state.pool, &ws.0.schema_name)
        .await
        .map_err(AppError::from)?;
    let mut db = DbContext::new(&mut *conn, prefix);
    let job_id = path.into_inner();
    let job = db::jobs::get(&mut db, &job_id)
        .await?
        .ok_or_else(|| AppError::JobNotFound(job_id.clone()))?;

    let execs = db::executions::list_for_job(&mut db, &job_id, None, 200).await?;

    let active = execs
        .iter()
        .filter(|e| {
            matches!(
                e.status.as_str(),
                "PENDING" | "QUEUED" | "RUNNING" | "RETRYING"
            )
        })
        .count();
    let succeeded = execs.iter().filter(|e| e.status == "SUCCESS").count();
    let failed = execs.iter().filter(|e| e.status == "FAILED").count();

    let health = if execs.is_empty() {
        "IDLE"
    } else if failed > succeeded {
        "FAILING"
    } else if failed > 0 {
        "DEGRADED"
    } else {
        "HEALTHY"
    };

    let last_exec = execs
        .iter()
        .find(|e| e.status == "SUCCESS" || e.status == "FAILED");

    Ok(HttpResponse::Ok().json(serde_json::json!({ "data": {
        "job_id": job.job_id,
        "endpoint": job.endpoint,
        "endpoint_type": job.endpoint_type,
        "trigger": job.trigger_type,
        "health": health,
        "version": job.version,
        "last_execution": last_exec.map(|e| serde_json::json!({
            "execution_id": e.execution_id,
            "status": e.status,
            "started_at": e.started_at,
            "completed_at": e.completed_at,
            "attempt_number": e.attempt_count,
        })),
        "active_executions": {
            "pending": execs.iter().filter(|e| e.status == "PENDING" || e.status == "QUEUED").count(),
            "running": execs.iter().filter(|e| e.status == "RUNNING" || e.status == "RETRYING").count(),
            "total": active,
        },
        "cron": if job.trigger_type == "CRON" { Some(serde_json::json!({
            "expression": job.cron_expression,
            "next_run_at": job.cron_next_run_at,
            "last_tick_at": job.cron_last_tick_at,
        })) } else { None },
    }})))
}

pub async fn versions(
    state: web::Data<AppState>,
    _auth: AuthenticatedRequest,
    ws: Workspace,
    path: web::Path<String>,
) -> Result<HttpResponse, AppError> {
    let prefix = state.prefix();
    let mut conn = kronos_common::db::scoped::scoped_connection(&state.pool, &ws.0.schema_name)
        .await
        .map_err(AppError::from)?;
    let mut db = DbContext::new(&mut *conn, prefix);
    let job_id = path.into_inner();
    let _ = db::jobs::get(&mut db, &job_id)
        .await?
        .ok_or_else(|| AppError::JobNotFound(job_id.clone()))?;

    let versions = db::jobs::get_versions(&mut db, &job_id).await?;
    let items: Vec<serde_json::Value> = versions.into_iter().map(|j| job_summary(&j)).collect();

    Ok(HttpResponse::Ok().json(serde_json::json!({ "data": items })))
}

pub async fn list_executions(
    state: web::Data<AppState>,
    _auth: AuthenticatedRequest,
    ws: Workspace,
    path: web::Path<String>,
    params: web::Query<PaginationParams>,
) -> Result<HttpResponse, AppError> {
    let prefix = state.prefix();
    let mut conn = kronos_common::db::scoped::scoped_connection(&state.pool, &ws.0.schema_name)
        .await
        .map_err(AppError::from)?;
    let mut db = DbContext::new(&mut *conn, prefix);
    let job_id = path.into_inner();
    let _ = db::jobs::get(&mut db, &job_id)
        .await?
        .ok_or_else(|| AppError::JobNotFound(job_id.clone()))?;

    let limit = params.effective_limit();
    let cursor = params.decode_cursor();
    let items =
        db::executions::list_for_job(&mut db, &job_id, cursor.as_deref(), limit + 1).await?;

    let has_more = items.len() as i64 > limit;
    let items: Vec<_> = items.into_iter().take(limit as usize).collect();
    let next_cursor = if has_more {
        items.last().map(|e| encode_cursor(&e.execution_id))
    } else {
        None
    };

    let data: Vec<serde_json::Value> = items
        .into_iter()
        .map(|e| {
            serde_json::json!({
                "execution_id": e.execution_id,
                "job_id": e.job_id,
                "status": e.status,
                "attempt_count": e.attempt_count,
                "max_attempts": e.max_attempts,
                "input": e.input,
                "output": e.output,
                "run_at": e.run_at,
                "started_at": e.started_at,
                "completed_at": e.completed_at,
                "created_at": e.created_at,
            })
        })
        .collect();

    Ok(HttpResponse::Ok().json(PaginatedResponse {
        data,
        cursor: next_cursor,
    }))
}

fn job_response(
    job: &kronos_common::models::Job,
    exec: Option<&kronos_common::models::Execution>,
) -> serde_json::Value {
    let mut v = job_summary(job);
    if let Some(e) = exec {
        v.as_object_mut().unwrap().insert(
            "execution".into(),
            serde_json::json!({
                "execution_id": e.execution_id,
                "status": e.status,
                "created_at": e.created_at,
            }),
        );
    }
    v
}

fn job_summary(job: &kronos_common::models::Job) -> serde_json::Value {
    serde_json::json!({
        "job_id": job.job_id,
        "endpoint": job.endpoint,
        "endpoint_type": job.endpoint_type,
        "trigger": job.trigger_type,
        "status": job.status,
        "version": job.version,
        "idempotency_key": job.idempotency_key,
        "input": job.input,
        "run_at": job.run_at,
        "cron": job.cron_expression,
        "timezone": job.cron_timezone,
        "starts_at": job.cron_starts_at,
        "ends_at": job.cron_ends_at,
        "next_run_at": job.cron_next_run_at,
        "created_at": job.created_at,
    })
}
