use crate::extractors::{AuthenticatedRequest, Workspace};
use crate::router::AppState;
use actix_web::{web, HttpResponse};
use chrono::Utc;
use kronos_common::{
    db,
    error::AppError,
    models::job::{CreateJob, TriggerType, UpdateJob},
    pagination::{encode_cursor, PaginatedResponse, PaginationParams},
};
use kronos_common::metrics as m;
use uuid::Uuid;

pub async fn create(
    state: web::Data<AppState>,
    _auth: AuthenticatedRequest,
    ws: Workspace,
    body: web::Json<CreateJob>,
) -> Result<HttpResponse, AppError> {
    let trigger = TriggerType::from_str_val(&body.trigger)
        .ok_or_else(|| AppError::InvalidRequest(format!("Invalid trigger: {}", body.trigger)))?;

    let mut conn = kronos_common::db::scoped::scoped_connection(&state.pool, &ws.0.schema_name)
        .await
        .map_err(AppError::from)?;

    let ep = db::endpoints::get(&mut *conn, &body.endpoint)
        .await?
        .ok_or_else(|| AppError::EndpointNotFound(body.endpoint.clone()))?;

    let retry_policy = ep.get_retry_policy();

    if let Some(ref ps_name) = ep.payload_spec_ref {
        if let Some(ref input) = body.input {
            let spec = db::payload_specs::get(&mut *conn, ps_name)
                .await?
                .ok_or_else(|| AppError::InvalidPayloadSpecRef(ps_name.clone()))?;
            validate_input(input, &spec.schema_json)?;
        }
    }

    if let Some(ref key) = body.idempotency_key {
        if let Some(existing) =
            db::jobs::get_by_idempotency(&mut *conn, &body.endpoint, key).await?
        {
            let exec = db::executions::get_for_job(&mut *conn, &existing.job_id).await?;
            return Ok(HttpResponse::Ok()
                .json(serde_json::json!({ "data": job_response(&existing, exec.as_ref()) })));
        }
    }

    // Drop the scoped connection before starting transactions for IMMEDIATE/DELAYED
    drop(conn);

    match trigger {
        TriggerType::IMMEDIATE => {
            let generated_key;
            let key = match body.idempotency_key.as_deref() {
                Some(k) => k,
                None => {
                    generated_key = Uuid::new_v4().to_string();
                    &generated_key
                }
            };

            let mut tx = kronos_common::db::scoped::scoped_transaction(
                &state.pool,
                &ws.0.schema_name,
            )
            .await
            .map_err(AppError::from)?;

            let result = db::jobs::create_immediate(
                &mut *tx,
                &body.endpoint,
                &ep.endpoint_type,
                key,
                body.input.as_ref(),
                retry_policy.max_attempts,
            )
            .await
            .map_err(|e| match e {
                sqlx::Error::Database(ref db_err) if db_err.constraint().is_some() => {
                    AppError::Conflict("Job with this idempotency key already exists".into())
                }
                _ => AppError::from(e),
            })?;

            tx.commit().await.map_err(AppError::from)?;

            metrics::counter!(m::JOBS_CREATED_TOTAL,
                "trigger_type" => "IMMEDIATE",
                "endpoint" => body.endpoint.clone(),
                "schema" => ws.0.schema_name.clone(),
            )
            .increment(1);

            Ok(HttpResponse::Created().json(serde_json::json!({ "data": {
                "job_id": result.job.job_id,
                "endpoint": result.job.endpoint,
                "endpoint_type": result.job.endpoint_type,
                "trigger": result.job.trigger_type,
                "status": result.job.status,
                "version": result.job.version,
                "idempotency_key": result.job.idempotency_key,
                "input": result.job.input,
                "execution": {
                    "execution_id": result.execution_id,
                    "status": result.execution_status,
                    "created_at": result.execution_created_at,
                },
                "created_at": result.job.created_at,
            }})))
        }
        TriggerType::DELAYED => {
            let key = body.idempotency_key.as_deref().ok_or_else(|| {
                AppError::InvalidRequest("idempotency_key required for DELAYED jobs".into())
            })?;
            let run_at = body.run_at.ok_or_else(|| {
                AppError::InvalidRequest("run_at required for DELAYED jobs".into())
            })?;

            let mut tx = kronos_common::db::scoped::scoped_transaction(
                &state.pool,
                &ws.0.schema_name,
            )
            .await
            .map_err(AppError::from)?;

            let result = db::jobs::create_delayed(
                &mut *tx,
                &body.endpoint,
                &ep.endpoint_type,
                key,
                body.input.as_ref(),
                run_at,
                retry_policy.max_attempts,
            )
            .await?;

            tx.commit().await.map_err(AppError::from)?;

            metrics::counter!(m::JOBS_CREATED_TOTAL,
                "trigger_type" => "DELAYED",
                "endpoint" => body.endpoint.clone(),
                "schema" => ws.0.schema_name.clone(),
            )
            .increment(1);

            Ok(HttpResponse::Created().json(serde_json::json!({ "data": {
                "job_id": result.job.job_id,
                "endpoint": result.job.endpoint,
                "endpoint_type": result.job.endpoint_type,
                "trigger": result.job.trigger_type,
                "status": result.job.status,
                "version": result.job.version,
                "idempotency_key": result.job.idempotency_key,
                "input": result.job.input,
                "run_at": result.job.run_at,
                "execution": {
                    "execution_id": result.execution_id,
                    "status": result.execution_status,
                    "created_at": result.execution_created_at,
                },
                "created_at": result.job.created_at,
            }})))
        }
        TriggerType::CRON => {
            let cron_expr = body
                .cron
                .as_deref()
                .ok_or_else(|| AppError::InvalidRequest("cron required for CRON jobs".into()))?;
            let tz_str = body.timezone.as_deref().ok_or_else(|| {
                AppError::InvalidRequest("timezone required for CRON jobs".into())
            })?;

            let schedule: cron::Schedule = cron_expr
                .parse()
                .map_err(|e| AppError::InvalidCron(format!("{}", e)))?;

            let tz: chrono_tz::Tz = tz_str
                .parse()
                .map_err(|_| AppError::InvalidRequest(format!("Invalid timezone: {}", tz_str)))?;

            let starts_at = body.starts_at.unwrap_or_else(Utc::now);
            let next_run = compute_next_cron(&schedule, &tz, starts_at).ok_or_else(|| {
                AppError::InvalidCron("No upcoming run for this cron schedule".into())
            })?;

            let mut conn =
                kronos_common::db::scoped::scoped_connection(&state.pool, &ws.0.schema_name)
                    .await
                    .map_err(AppError::from)?;

            let job = db::jobs::create_cron(
                &mut *conn,
                &body.endpoint,
                &ep.endpoint_type,
                body.input.as_ref(),
                cron_expr,
                tz_str,
                Some(starts_at),
                body.ends_at,
                next_run,
            )
            .await?;

            metrics::counter!(m::JOBS_CREATED_TOTAL,
                "trigger_type" => "CRON",
                "endpoint" => body.endpoint.clone(),
                "schema" => ws.0.schema_name.clone(),
            )
            .increment(1);

            Ok(HttpResponse::Created().json(serde_json::json!({ "data": {
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
            }})))
        }
    }
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
    let items = db::jobs::list(&mut *conn, cursor.as_deref(), limit + 1).await?;

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
    let mut conn = kronos_common::db::scoped::scoped_connection(&state.pool, &ws.0.schema_name)
        .await
        .map_err(AppError::from)?;
    let job_id = path.into_inner();
    let job = db::jobs::get(&mut *conn, &job_id)
        .await?
        .ok_or_else(|| AppError::JobNotFound(job_id))?;
    let exec = db::executions::get_for_job(&mut *conn, &job.job_id).await?;
    Ok(HttpResponse::Ok().json(serde_json::json!({ "data": job_response(&job, exec.as_ref()) })))
}

pub async fn update(
    state: web::Data<AppState>,
    _auth: AuthenticatedRequest,
    ws: Workspace,
    path: web::Path<String>,
    body: web::Json<UpdateJob>,
) -> Result<HttpResponse, AppError> {
    let mut conn = kronos_common::db::scoped::scoped_connection(&state.pool, &ws.0.schema_name)
        .await
        .map_err(AppError::from)?;
    let job_id = path.into_inner();
    let old_job = db::jobs::get(&mut *conn, &job_id)
        .await?
        .ok_or_else(|| AppError::JobNotFound(job_id.clone()))?;

    if old_job.trigger_type != "CRON" {
        return Err(AppError::JobNotUpdatable(
            "Only CRON jobs can be updated".into(),
        ));
    }
    if old_job.status != "ACTIVE" {
        return Err(AppError::JobNotUpdatable("Job is not active".into()));
    }

    let cron_expr = body
        .cron
        .as_deref()
        .unwrap_or(old_job.cron_expression.as_deref().unwrap_or(""));
    let tz_str = body
        .timezone
        .as_deref()
        .unwrap_or(old_job.cron_timezone.as_deref().unwrap_or("UTC"));

    let schedule: cron::Schedule = cron_expr
        .parse()
        .map_err(|e| AppError::InvalidCron(format!("{}", e)))?;
    let tz: chrono_tz::Tz = tz_str
        .parse()
        .map_err(|_| AppError::InvalidRequest(format!("Invalid timezone: {}", tz_str)))?;

    let next_run = compute_next_cron(&schedule, &tz, Utc::now())
        .ok_or_else(|| AppError::InvalidCron("No upcoming run".into()))?;

    let mut new_job = old_job.clone();
    new_job.cron_expression = Some(cron_expr.to_string());
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

    let created = db::jobs::retire_and_replace(&mut *tx, &job_id, &new_job).await?;

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
    let mut conn = kronos_common::db::scoped::scoped_connection(&state.pool, &ws.0.schema_name)
        .await
        .map_err(AppError::from)?;
    let job_id = path.into_inner();
    let job = db::jobs::get(&mut *conn, &job_id)
        .await?
        .ok_or_else(|| AppError::JobNotFound(job_id.clone()))?;

    if job.status == "RETIRED" {
        return Err(AppError::Conflict("Job is already retired".into()));
    }

    if job.trigger_type != "CRON" {
        db::executions::cancel_pending_for_job(&mut *conn, &job_id).await?;
    }

    let cancelled = db::jobs::cancel(&mut *conn, &job_id)
        .await?
        .ok_or_else(|| AppError::Conflict("Job could not be cancelled".into()))?;

    Ok(HttpResponse::Ok().json(serde_json::json!({ "data": job_summary(&cancelled) })))
}

pub async fn status(
    state: web::Data<AppState>,
    _auth: AuthenticatedRequest,
    ws: Workspace,
    path: web::Path<String>,
) -> Result<HttpResponse, AppError> {
    let mut conn = kronos_common::db::scoped::scoped_connection(&state.pool, &ws.0.schema_name)
        .await
        .map_err(AppError::from)?;
    let job_id = path.into_inner();
    let job = db::jobs::get(&mut *conn, &job_id)
        .await?
        .ok_or_else(|| AppError::JobNotFound(job_id.clone()))?;

    let execs = db::executions::list_for_job(&mut *conn, &job_id, None, 200).await?;

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
    let mut conn = kronos_common::db::scoped::scoped_connection(&state.pool, &ws.0.schema_name)
        .await
        .map_err(AppError::from)?;
    let job_id = path.into_inner();
    let _ = db::jobs::get(&mut *conn, &job_id)
        .await?
        .ok_or_else(|| AppError::JobNotFound(job_id.clone()))?;

    let versions = db::jobs::get_versions(&mut *conn, &job_id).await?;
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
    let mut conn = kronos_common::db::scoped::scoped_connection(&state.pool, &ws.0.schema_name)
        .await
        .map_err(AppError::from)?;
    let job_id = path.into_inner();
    let _ = db::jobs::get(&mut *conn, &job_id)
        .await?
        .ok_or_else(|| AppError::JobNotFound(job_id.clone()))?;

    let limit = params.effective_limit();
    let cursor = params.decode_cursor();
    let items =
        db::executions::list_for_job(&mut *conn, &job_id, cursor.as_deref(), limit + 1).await?;

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

fn validate_input(input: &serde_json::Value, schema: &serde_json::Value) -> Result<(), AppError> {
    let compiled = jsonschema::JSONSchema::compile(schema)
        .map_err(|e| AppError::InvalidSchema(format!("{}", e)))?;

    if let Err(errors) = compiled.validate(input) {
        let msgs: Vec<String> = errors.map(|e| e.to_string()).collect();
        return Err(AppError::InputValidationFailed(msgs.join("; ")));
    }
    Ok(())
}

fn compute_next_cron(
    schedule: &cron::Schedule,
    tz: &chrono_tz::Tz,
    after: chrono::DateTime<Utc>,
) -> Option<chrono::DateTime<Utc>> {
    let after_tz = after.with_timezone(tz);
    schedule
        .after(&after_tz)
        .next()
        .map(|dt| dt.with_timezone(&Utc))
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
