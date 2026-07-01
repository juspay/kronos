use crate::extractors::{AuthenticatedRequest, Workspace};
use crate::router::AppState;
use actix_web::{web, HttpResponse};
use chrono::{DateTime, Utc};
use kronos_common::metrics as m;
use kronos_common::{
    db,
    db::DbContext,
    error::AppError,
    models::endpoint::EndpointType,
    models::job::{CreateJob, JobStatus, TriggerType, UpdateJob},
    models::pg_cron_expr::PgCronExpr,
    pagination::{encode_cursor, PaginatedResponse, PaginationParams},
};
use uuid::Uuid;

pub async fn create(
    state: web::Data<AppState>,
    _auth: AuthenticatedRequest,
    ws: Workspace,
    body: web::Json<CreateJob>,
) -> Result<HttpResponse, AppError> {
    let prefix = state.prefix();

    let trigger = TriggerType::from_str_val(&body.trigger)
        .ok_or_else(|| AppError::InvalidRequest(format!("Invalid trigger: {}", body.trigger)))?;

    let mut conn = kronos_common::db::scoped::scoped_connection(&state.pool, &ws.0.schema_name)
        .await
        .map_err(AppError::from)?;
    let mut db = DbContext::new(&mut *conn, prefix);

    let ep = db::endpoints::get(&mut db, &body.endpoint)
        .await?
        .ok_or_else(|| AppError::EndpointNotFound(body.endpoint.clone()))?;

    // INTERNAL endpoints back kronos-driven jobs (today: the dogfooded reaper)
    // and are not user-creatable — see `handlers::endpoints::create`. The same
    // invariant has to hold on the job side, or a user could stack their own
    // jobs against an internal endpoint (extra reaper sweeps, or one-off
    // IMMEDIATE/DELAYED reaper invocations).
    if EndpointType::from_str_val(&ep.endpoint_type) == Some(EndpointType::INTERNAL) {
        return Err(AppError::InvalidRequest(format!(
            "Endpoint '{}' is internal and cannot be used for user-created jobs",
            body.endpoint
        )));
    }

    let retry_policy = ep.get_retry_policy();

    if let Some(ref ps_name) = ep.payload_spec_ref {
        if let Some(ref input) = body.input {
            let spec = db::payload_specs::get(&mut db, ps_name)
                .await?
                .ok_or_else(|| AppError::InvalidPayloadSpecRef(ps_name.clone()))?;
            validate_input(input, &spec.schema_json)?;
        }
    }

    if let Some(ref key) = body.idempotency_key {
        if let Some(existing) = db::jobs::get_by_idempotency(&mut db, &body.endpoint, key).await? {
            let exec = db::executions::get_for_job(&mut db, &existing.job_id).await?;
            return Ok(HttpResponse::Ok()
                .json(serde_json::json!({ "data": job_response(&existing, exec.as_ref()) })));
        }
    }

    // Drop the scoped connection (and its DbContext) before starting transactions
    drop(db);
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

            let mut tx =
                kronos_common::db::scoped::scoped_transaction(&state.pool, &ws.0.schema_name)
                    .await
                    .map_err(AppError::from)?;
            let mut db = DbContext::new(&mut *tx, prefix);

            let result = db::jobs::create_immediate(
                &mut db,
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

            drop(db);
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

            let mut tx =
                kronos_common::db::scoped::scoped_transaction(&state.pool, &ws.0.schema_name)
                    .await
                    .map_err(AppError::from)?;
            let mut db = DbContext::new(&mut *tx, prefix);

            let result = db::jobs::create_delayed(
                &mut db,
                &body.endpoint,
                &ep.endpoint_type,
                key,
                body.input.as_ref(),
                run_at,
                retry_policy.max_attempts,
            )
            .await?;

            drop(db);
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
                .as_ref()
                .ok_or_else(|| AppError::InvalidRequest("cron required for CRON jobs".into()))?;
            let tz_str = body.timezone.as_deref().ok_or_else(|| {
                AppError::InvalidRequest("timezone required for CRON jobs".into())
            })?;

            let schedule = cron_expr.to_schedule();

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
            let mut db = DbContext::new(&mut *conn, prefix);

            let job = db::jobs::create_cron(
                &mut db,
                &body.endpoint,
                &ep.endpoint_type,
                body.input.as_ref(),
                cron_expr.as_str(),
                tz_str,
                Some(starts_at),
                body.ends_at,
                next_run,
            )
            .await?;

            if let Err(e) = db::jobs::register_pg_cron(
                &state.pool,
                prefix,
                &ws.0.schema_name,
                &job.job_id,
                cron_expr.as_str(),
            )
            .await
            {
                tracing::error!(job_id = %job.job_id, "Failed to register pg_cron job: {}", e);
            }

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

fn blank_to_none(value: Option<String>) -> Option<String> {
    value.and_then(|v| {
        let trimmed = v.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    })
}

/// The last non-blank value for `key` across the raw query pairs. Scalar filters
/// (`job_id`, `endpoint`, the date bounds) are single-valued, so a repeated key
/// keeps the final occurrence; blanks (e.g. `?job_id=`) are treated as absent.
fn last_scalar(pairs: &[(String, String)], key: &str) -> Option<String> {
    pairs
        .iter()
        .filter(|(k, _)| k == key)
        .filter_map(|(_, v)| blank_to_none(Some(v.clone())))
        .last()
}

/// Collects a validated, de-duplicated enum list from the query pairs for `key`.
///
/// Liberal in what it accepts: both repeated params (`?status=A&status=B`, which
/// the generated Smithy SDK emits for a list-typed `@httpQuery`) and a single
/// comma-separated value (`?status=A,B`, which the dashboard emits). Blank tokens
/// are skipped; an invalid token fails the whole request with a 400 rather than
/// silently dropping rows.
fn parse_filter_list<T: PartialEq>(
    pairs: &[(String, String)],
    key: &str,
    parse: impl Fn(&str) -> Option<T>,
    label: &str,
) -> Result<Vec<T>, AppError> {
    let mut out: Vec<T> = Vec::new();
    for (_, value) in pairs.iter().filter(|(k, _)| k == key) {
        for token in value.split(',') {
            let token = token.trim();
            if token.is_empty() {
                continue;
            }
            let parsed = parse(token)
                .ok_or_else(|| AppError::InvalidRequest(format!("Invalid {label}: {token}")))?;
            if !out.contains(&parsed) {
                out.push(parsed);
            }
        }
    }
    Ok(out)
}

/// Parses an optional RFC-3339 datetime query value to UTC; blank == absent.
fn parse_datetime(value: Option<String>, label: &str) -> Result<Option<DateTime<Utc>>, AppError> {
    match blank_to_none(value) {
        Some(s) => {
            let dt = DateTime::parse_from_rfc3339(&s)
                .map_err(|_| AppError::InvalidRequest(format!("Invalid {label}: {s}")))?;
            Ok(Some(dt.with_timezone(&Utc)))
        }
        None => Ok(None),
    }
}

/// Parses and validates the server-side jobs-list filters from the raw query
/// pairs (parsed alongside [`PaginationParams`]). Enum filters are validated up
/// front so a typo surfaces as a 400 rather than silently returning zero rows;
/// `endpoint` is a free-text substring search, so it needs no validation.
///
/// Taking the raw pairs (rather than a `serde`-derived struct) is deliberate:
/// the multi-value filters must accept a key appearing more than once, which
/// `serde_urlencoded` cannot fold into a `Vec` for an `Option<String>` field.
fn parse_job_filters(pairs: &[(String, String)]) -> Result<db::jobs::JobFilters, AppError> {
    let created_after = parse_datetime(last_scalar(pairs, "created_after"), "created_after")?;
    let created_before = parse_datetime(last_scalar(pairs, "created_before"), "created_before")?;
    if let (Some(a), Some(b)) = (created_after, created_before) {
        if a > b {
            return Err(AppError::InvalidRequest(
                "created_after must not be after created_before".into(),
            ));
        }
    }
    Ok(db::jobs::JobFilters {
        job_id: last_scalar(pairs, "job_id"),
        status: parse_filter_list(pairs, "status", JobStatus::from_str_val, "status")?,
        trigger: parse_filter_list(pairs, "trigger_type", TriggerType::from_str_val, "trigger")?,
        endpoint: last_scalar(pairs, "endpoint"),
        endpoint_type: parse_filter_list(
            pairs,
            "endpoint_type",
            EndpointType::from_str_val,
            "endpoint_type",
        )?,
        created_after,
        created_before,
    })
}

pub async fn list(
    state: web::Data<AppState>,
    _auth: AuthenticatedRequest,
    ws: Workspace,
    params: web::Query<PaginationParams>,
    // Raw pairs rather than a typed struct: multi-value filters may repeat a key
    // (`?status=A&status=B`), which the SDK emits and `serde_urlencoded` cannot
    // fold into a `Vec`. `parse_job_filters` also still accepts `?status=A,B`.
    raw_query: web::Query<Vec<(String, String)>>,
) -> Result<HttpResponse, AppError> {
    let prefix = state.prefix();
    let mut conn = kronos_common::db::scoped::scoped_connection(&state.pool, &ws.0.schema_name)
        .await
        .map_err(AppError::from)?;
    let mut db = DbContext::new(&mut *conn, prefix);
    let limit = params.effective_limit();
    let cursor = params.decode_cursor();
    let filters = parse_job_filters(&raw_query.into_inner())?;
    let items = db::jobs::list(&mut db, cursor.as_deref(), limit + 1, &filters).await?;

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

    let next_run = compute_next_cron(&schedule, &tz, Utc::now())
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
    tx.commit().await.map_err(AppError::from)?;

    if let Err(e) = db::jobs::unregister_pg_cron(&state.pool, &ws.0.schema_name, &job_id).await {
        tracing::error!(job_id = %job_id, "Failed to unregister old pg_cron job: {}", e);
    }
    if let Err(e) = db::jobs::register_pg_cron(
        &state.pool,
        prefix,
        &ws.0.schema_name,
        &created.job_id,
        cron_expr.as_str(),
    )
    .await
    {
        tracing::error!(job_id = %created.job_id, "Failed to register new pg_cron job: {}", e);
    }

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
    let prefix = state.prefix();
    let mut conn = kronos_common::db::scoped::scoped_connection(&state.pool, &ws.0.schema_name)
        .await
        .map_err(AppError::from)?;
    let mut db = DbContext::new(&mut *conn, prefix);
    let job_id = path.into_inner();
    let job = db::jobs::get(&mut db, &job_id)
        .await?
        .ok_or_else(|| AppError::JobNotFound(job_id.clone()))?;

    if job.status == "RETIRED" {
        return Err(AppError::Conflict("Job is already retired".into()));
    }
    // INTERNAL jobs are kronos-managed (today: the dogfooded reaper). Cancelling
    // one would stop the reaper from sweeping this workspace, with no surviving
    // bootstrap path to bring it back.
    if EndpointType::from_str_val(&job.endpoint_type) == Some(EndpointType::INTERNAL) {
        return Err(AppError::Conflict(
            "Internal kronos jobs cannot be cancelled through the API".into(),
        ));
    }

    if job.trigger_type != "CRON" {
        db::executions::cancel_pending_for_job(&mut db, &job_id).await?;
    }

    let cancelled = db::jobs::cancel(&mut db, &job_id)
        .await?
        .ok_or_else(|| AppError::Conflict("Job could not be cancelled".into()))?;

    drop(db);

    if job.trigger_type == "CRON" {
        if let Err(e) = db::jobs::unregister_pg_cron(&state.pool, &ws.0.schema_name, &job_id).await
        {
            tracing::error!(job_id = %job_id, "Failed to unregister pg_cron job: {}", e);
        }
    }

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

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds raw query pairs from `key=value` strings, mirroring what actix
    /// hands the handler as `web::Query<Vec<(String, String)>>`.
    fn pairs(kvs: &[(&str, &str)]) -> Vec<(String, String)> {
        kvs.iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    fn assert_invalid_request(result: Result<db::jobs::JobFilters, AppError>) {
        match result {
            Err(AppError::InvalidRequest(_)) => {}
            Err(_) => panic!("expected InvalidRequest"),
            Ok(_) => panic!("expected an error, got Ok"),
        }
    }

    #[test]
    fn parse_job_filters_carries_trimmed_job_id() {
        let f = parse_job_filters(&pairs(&[("job_id", "  job-42  ")])).unwrap();
        assert_eq!(f.job_id, Some("job-42".to_string()));
    }

    #[test]
    fn parse_job_filters_parses_comma_separated_enums() {
        let f = parse_job_filters(&pairs(&[
            ("status", "ACTIVE,RETIRED"),
            ("trigger_type", "CRON,DELAYED"),
            ("endpoint", "notify"),
            ("endpoint_type", "HTTP,INTERNAL"),
        ]))
        .unwrap();
        assert_eq!(f.status, vec![JobStatus::ACTIVE, JobStatus::RETIRED]);
        assert_eq!(f.trigger, vec![TriggerType::CRON, TriggerType::DELAYED]);
        assert_eq!(f.endpoint, Some("notify".to_string()));
        assert_eq!(
            f.endpoint_type,
            vec![EndpointType::HTTP, EndpointType::INTERNAL]
        );
    }

    #[test]
    fn parse_job_filters_parses_repeated_params() {
        // The generated Smithy SDK emits one entry per list value.
        let f = parse_job_filters(&pairs(&[
            ("status", "ACTIVE"),
            ("status", "RETIRED"),
            ("endpoint_type", "HTTP"),
            ("endpoint_type", "INTERNAL"),
        ]))
        .unwrap();
        assert_eq!(f.status, vec![JobStatus::ACTIVE, JobStatus::RETIRED]);
        assert_eq!(
            f.endpoint_type,
            vec![EndpointType::HTTP, EndpointType::INTERNAL]
        );
    }

    #[test]
    fn parse_job_filters_mixes_repeated_and_comma_separated() {
        let f = parse_job_filters(&pairs(&[
            ("status", "ACTIVE,RETIRED"),
            ("status", "ACTIVE"),
        ]))
        .unwrap();
        // Deduped across both the comma-split and the repeated occurrence.
        assert_eq!(f.status, vec![JobStatus::ACTIVE, JobStatus::RETIRED]);
    }

    #[test]
    fn parse_job_filters_scalar_takes_last_non_blank() {
        let f = parse_job_filters(&pairs(&[("job_id", "job-1"), ("job_id", "  job-2  ")])).unwrap();
        assert_eq!(f.job_id, Some("job-2".to_string()));
    }

    #[test]
    fn parse_job_filters_trims_dedupes_and_drops_blanks() {
        let f = parse_job_filters(&pairs(&[("status", " ACTIVE , ACTIVE ,, RETIRED ")])).unwrap();
        assert_eq!(f.status, vec![JobStatus::ACTIVE, JobStatus::RETIRED]);
    }

    #[test]
    fn parse_job_filters_empty_lists_when_absent() {
        let f = parse_job_filters(&pairs(&[])).unwrap();
        assert!(f.status.is_empty());
        assert!(f.trigger.is_empty());
        assert!(f.endpoint_type.is_empty());
        assert_eq!(f.job_id, None);
        assert_eq!(f.endpoint, None);
        assert_eq!(f.created_after, None);
        assert_eq!(f.created_before, None);
    }

    #[test]
    fn parse_job_filters_blank_values_are_absent() {
        // The dashboard sends empty params for an "All" selection.
        let f = parse_job_filters(&pairs(&[
            ("status", ""),
            ("job_id", "  "),
            ("endpoint", ""),
        ]))
        .unwrap();
        assert!(f.status.is_empty());
        assert_eq!(f.job_id, None);
        assert_eq!(f.endpoint, None);
    }

    #[test]
    fn parse_job_filters_parses_rfc3339_dates() {
        let f = parse_job_filters(&pairs(&[
            ("created_after", "2026-06-18T00:00:00Z"),
            ("created_before", "2026-06-24T23:59:59Z"),
        ]))
        .unwrap();
        assert!(f.created_after.is_some());
        assert!(f.created_before.is_some());
    }

    #[test]
    fn parse_job_filters_rejects_bad_enum_token() {
        assert_invalid_request(parse_job_filters(&pairs(&[("status", "ACTIVE,BOGUS")])));
    }

    #[test]
    fn parse_job_filters_rejects_bad_date() {
        assert_invalid_request(parse_job_filters(&pairs(&[("created_after", "yesterday")])));
    }

    #[test]
    fn parse_job_filters_rejects_inverted_date_range() {
        assert_invalid_request(parse_job_filters(&pairs(&[
            ("created_after", "2026-06-24T00:00:00Z"),
            ("created_before", "2026-06-18T00:00:00Z"),
        ])));
    }
}
