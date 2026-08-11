//! Job creation and cancellation, shared by the REST handler and the library
//! client.

use super::{ServiceError, WorkspaceRef};
use crate::db::{self, scoped, DbContext};
use crate::models::endpoint::EndpointType;
use crate::models::pg_cron_expr::PgCronExpr;
use crate::models::{Execution, Job};
use chrono::{DateTime, Utc};

/// How the job should fire, in the shape a caller supplies it.
///
/// The per-trigger required fields are `Option` here and validated *inside*
/// [`create_job`], deliberately: REST checks the endpoint (404) before it checks
/// the request body (400), and moving that validation up into the adapter would
/// silently flip the status code for a request that gets both wrong. Owning the
/// order here keeps it identical in both modes.
pub enum TriggerSpec {
    Immediate,
    Delayed {
        run_at: Option<DateTime<Utc>>,
    },
    Cron {
        expression: Option<PgCronExpr>,
        timezone: Option<String>,
        starts_at: Option<DateTime<Utc>>,
        ends_at: Option<DateTime<Utc>>,
        /// Seeds the reported `cron_next_run_at`. `None` computes it from the
        /// schedule (what REST does); `Some` lets a caller that already knows
        /// its first run keep it. Either way the actual firing is driven by
        /// pg_cron plus the `cron_starts_at` guard, never by this column.
        next_run_at: Option<DateTime<Utc>>,
    },
}

pub struct CreateJobRequest<'a> {
    pub endpoint: &'a str,
    pub trigger: TriggerSpec,
    pub input: Option<&'a serde_json::Value>,
    /// Already resolved by the adapter. REST substitutes a generated UUID for a
    /// keyless IMMEDIATE job; the library passes the caller's `Option` straight
    /// through so a missing key is stored as SQL `NULL`.
    pub idempotency_key: Option<&'a str>,
    /// `None` or a non-positive value falls back to the endpoint's retry policy.
    pub max_attempts: Option<i64>,
    /// Reject a DELAYED job that arrives without an idempotency key. REST sets
    /// this (a delayed job the caller cannot re-address is not retryable); the
    /// library leaves it off, where a keyless DELAYED job is stored with a NULL
    /// key and has always been allowed.
    pub require_idempotency_key: bool,
}

/// The execution created alongside a new job. CRON jobs have none — pg_cron
/// materializes their executions on each tick.
pub struct ExecutionSummary {
    pub execution_id: String,
    pub status: String,
    pub created_at: DateTime<Utc>,
}

/// A trigger whose required fields have been checked and whose schedule has been
/// resolved — everything the write phase needs, with no remaining `Option`s.
enum TriggerPlan {
    Immediate,
    Delayed {
        run_at: DateTime<Utc>,
    },
    Cron {
        expression: PgCronExpr,
        timezone: String,
        starts_at: DateTime<Utc>,
        ends_at: Option<DateTime<Utc>>,
        next_run: DateTime<Utc>,
    },
}

pub enum CreateOutcome {
    Created {
        job: Job,
        execution: Option<ExecutionSummary>,
    },
    /// The idempotency key was already used on this endpoint; nothing was
    /// written and the original job is returned.
    ///
    /// The full `Execution` is boxed so this variant does not inflate the size
    /// of every `CreateOutcome`; the common `Created` arm only carries a
    /// summary.
    AlreadyExists {
        job: Job,
        execution: Option<Box<Execution>>,
    },
}

/// Create a job in `ws`, enforcing every workspace-mutation invariant:
/// endpoint must exist and not be INTERNAL, input must satisfy the endpoint's
/// payload spec, a reused idempotency key short-circuits, and the job row, its
/// execution and any pg_cron registration all land on one transaction.
pub async fn create_job(
    ws: WorkspaceRef<'_>,
    req: CreateJobRequest<'_>,
) -> Result<CreateOutcome, ServiceError> {
    // Phase 1 — read-only checks. Held on a plain scoped connection so the
    // transaction in phase 2 is as short as possible.
    let mut conn = scoped::scoped_connection(ws.pool, ws.schema_name).await?;
    let mut db = DbContext::new(&mut *conn, ws.prefix);

    let ep = db::endpoints::get(&mut db, req.endpoint)
        .await?
        .ok_or_else(|| ServiceError::EndpointNotFound(req.endpoint.to_string()))?;

    // INTERNAL endpoints back kronos-driven jobs (today: the dogfooded reaper).
    // Letting a user stack jobs on one means extra reaper sweeps, or a one-off
    // IMMEDIATE invocation of an endpoint that expects to be CRON-driven.
    if EndpointType::from_str_val(&ep.endpoint_type) == Some(EndpointType::INTERNAL) {
        return Err(ServiceError::InternalEndpoint(req.endpoint.to_string()));
    }

    // Per-job override wins when provided and positive; otherwise the endpoint's
    // policy. A caller passing 0 means "unset", not "never retry".
    let max_attempts = req
        .max_attempts
        .filter(|&n| n > 0)
        .unwrap_or_else(|| ep.get_retry_policy().max_attempts);

    if let Some(ref ps_name) = ep.payload_spec_ref {
        if let Some(input) = req.input {
            let spec = db::payload_specs::get(&mut db, ps_name)
                .await?
                .ok_or_else(|| ServiceError::InvalidPayloadSpecRef(ps_name.clone()))?;
            validate_input(input, &spec.schema_json)?;
        }
    }

    if let Some(key) = req.idempotency_key {
        if let Some(existing) = db::jobs::get_by_idempotency(&mut db, req.endpoint, key).await? {
            let execution = db::executions::get_for_job(&mut db, &existing.job_id).await?;
            return Ok(CreateOutcome::AlreadyExists {
                job: existing,
                execution: execution.map(Box::new),
            });
        }
    }

    drop(db);
    drop(conn);

    // Trigger-shape validation runs *after* the endpoint checks above, matching
    // the order REST has always used. Everything that can fail without touching
    // the DB is resolved before the transaction opens, so a bad cron expression
    // or timezone never holds one.
    let plan = match req.trigger {
        TriggerSpec::Immediate => TriggerPlan::Immediate,
        TriggerSpec::Delayed { run_at } => {
            if req.require_idempotency_key && req.idempotency_key.is_none() {
                return Err(ServiceError::InvalidRequest(
                    "idempotency_key required for DELAYED jobs".into(),
                ));
            }
            let run_at = run_at.ok_or_else(|| {
                ServiceError::InvalidRequest("run_at required for DELAYED jobs".into())
            })?;
            TriggerPlan::Delayed { run_at }
        }
        TriggerSpec::Cron {
            expression,
            timezone,
            starts_at,
            ends_at,
            next_run_at,
        } => {
            let expression = expression.ok_or_else(|| {
                ServiceError::InvalidRequest("cron required for CRON jobs".into())
            })?;
            let timezone = timezone.ok_or_else(|| {
                ServiceError::InvalidRequest("timezone required for CRON jobs".into())
            })?;
            let tz: chrono_tz::Tz = timezone.parse().map_err(|_| {
                ServiceError::InvalidRequest(format!("Invalid timezone: {timezone}"))
            })?;
            let starts_at = starts_at.unwrap_or_else(Utc::now);
            let next_run = match next_run_at {
                Some(t) => t,
                None => compute_next_cron(&expression.to_schedule(), &tz, starts_at).ok_or_else(
                    || ServiceError::InvalidCron("No upcoming run for this cron schedule".into()),
                )?,
            };
            TriggerPlan::Cron {
                expression,
                timezone,
                starts_at,
                ends_at,
                next_run,
            }
        }
    };

    // Phase 2 — every write on one transaction. `create_immediate` and
    // `create_delayed` each run two INSERTs (jobs, then executions); on an
    // autocommit connection a failure between them strands an ACTIVE job with no
    // execution, which lists but never runs.
    let mut tx = scoped::scoped_transaction(ws.pool, ws.schema_name).await?;
    let mut db = DbContext::new(&mut *tx, ws.prefix);

    let outcome = match plan {
        TriggerPlan::Immediate => {
            let result = db::jobs::create_immediate(
                &mut db,
                req.endpoint,
                &ep.endpoint_type,
                req.idempotency_key,
                req.input,
                max_attempts,
            )
            .await
            .map_err(duplicate_key_as_conflict)?;
            drop(db);
            CreateOutcome::Created {
                execution: Some(ExecutionSummary {
                    execution_id: result.execution_id,
                    status: result.execution_status,
                    created_at: result.execution_created_at,
                }),
                job: result.job,
            }
        }
        TriggerPlan::Delayed { run_at } => {
            let result = db::jobs::create_delayed(
                &mut db,
                req.endpoint,
                &ep.endpoint_type,
                req.idempotency_key,
                req.input,
                run_at,
                max_attempts,
            )
            .await?;
            drop(db);
            CreateOutcome::Created {
                execution: Some(ExecutionSummary {
                    execution_id: result.execution_id,
                    status: result.execution_status,
                    created_at: result.execution_created_at,
                }),
                job: result.job,
            }
        }
        TriggerPlan::Cron {
            expression,
            timezone,
            starts_at,
            ends_at,
            next_run,
        } => {
            let job = db::jobs::create_cron(
                &mut db,
                req.endpoint,
                &ep.endpoint_type,
                req.input,
                expression.as_str(),
                &timezone,
                Some(starts_at),
                ends_at,
                next_run,
            )
            .await?;
            drop(db);

            // On the same transaction as the row write, so the persisted job and
            // its pg_cron schedule commit (or roll back) together. A job row
            // without a pg_cron entry never ticks; an entry without a row fires
            // no-op inserts forever.
            db::jobs::register_pg_cron_conn(
                &mut tx,
                ws.prefix,
                ws.schema_name,
                &job.job_id,
                expression.as_str(),
            )
            .await?;

            CreateOutcome::Created {
                job,
                execution: None,
            }
        }
    };

    tx.commit().await?;
    Ok(outcome)
}

/// Cancel a job: retire the row, cancel any pending executions, and unschedule
/// its pg_cron entry — all on one transaction.
pub async fn cancel_job(ws: WorkspaceRef<'_>, job_id: &str) -> Result<Job, ServiceError> {
    let mut tx = scoped::scoped_transaction(ws.pool, ws.schema_name).await?;
    let mut db = DbContext::new(&mut *tx, ws.prefix);

    let job = db::jobs::get(&mut db, job_id)
        .await?
        .ok_or_else(|| ServiceError::JobNotFound(job_id.to_string()))?;

    if job.status == "RETIRED" {
        return Err(ServiceError::Conflict("Job is already retired".into()));
    }
    // Cancelling the reaper would stop this workspace's CRON sweep with no
    // surviving bootstrap path to bring it back.
    if EndpointType::from_str_val(&job.endpoint_type) == Some(EndpointType::INTERNAL) {
        return Err(ServiceError::Conflict(
            "Internal kronos jobs cannot be cancelled through the API".into(),
        ));
    }

    if job.trigger_type != "CRON" {
        db::executions::cancel_pending_for_job(&mut db, job_id).await?;
    }

    let cancelled = db::jobs::cancel(&mut db, job_id)
        .await?
        .ok_or_else(|| ServiceError::Conflict("Job could not be cancelled".into()))?;

    drop(db);

    // Same transaction as the status flip: a RETIRED job whose pg_cron entry
    // survived is a permanent leak, since later sweeps only look at ACTIVE jobs.
    if job.trigger_type == "CRON" {
        db::jobs::unregister_pg_cron_conn(&mut tx, ws.schema_name, job_id).await?;
    }

    tx.commit().await?;
    Ok(cancelled)
}

/// A unique-index violation on insert means a concurrent request won the race
/// for this idempotency key — a conflict, not an internal error.
fn duplicate_key_as_conflict(e: sqlx::Error) -> ServiceError {
    match e {
        sqlx::Error::Database(ref db_err) if db_err.constraint().is_some() => {
            ServiceError::Conflict("Job with this idempotency key already exists".into())
        }
        other => ServiceError::Db(other),
    }
}

pub fn validate_input(
    input: &serde_json::Value,
    schema: &serde_json::Value,
) -> Result<(), ServiceError> {
    let compiled = jsonschema::JSONSchema::compile(schema)
        .map_err(|e| ServiceError::InvalidSchema(format!("{}", e)))?;

    if let Err(errors) = compiled.validate(input) {
        let msgs: Vec<String> = errors.map(|e| e.to_string()).collect();
        return Err(ServiceError::InputValidationFailed(msgs.join("; ")));
    }
    Ok(())
}

/// First scheduled run strictly after `after`, evaluated in the job's timezone
/// and returned as UTC.
pub fn compute_next_cron(
    schedule: &cron::Schedule,
    tz: &chrono_tz::Tz,
    after: DateTime<Utc>,
) -> Option<DateTime<Utc>> {
    let after_tz = after.with_timezone(tz);
    schedule
        .after(&after_tz)
        .next()
        .map(|dt| dt.with_timezone(&Utc))
}
