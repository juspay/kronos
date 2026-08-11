//! Shared workspace-mutation logic, sitting above `db::` and below both
//! adapters.
//!
//! Kronos has three entry points but only ever had two implementations of the
//! business logic: `KronosHttpClient` rides on the REST handler and so cannot
//! drift, while `KronosLibraryClient` re-implemented endpoint lookup, guards,
//! validation, idempotency, transaction boundaries and pg_cron registration by
//! hand. Everything below the raw SQL was shared; everything above it was
//! duplicated, and the two copies drifted (see issue #55).
//!
//! This module is the single core both adapters call. It owns the semantics —
//! what a job creation *means* — and returns neutral types. Each adapter stays
//! thin: the REST handler maps [`ServiceError`] to [`AppError`] and renders
//! HTTP, the library client maps it to `anyhow`. Parity stops being a manual
//! discipline and becomes a property of the call graph.

pub mod jobs;

use crate::error::AppError;
use sqlx::PgPool;

/// The workspace a service call operates on: which pool, which tenant schema,
/// and which table prefix that tenant's tables carry.
#[derive(Clone, Copy)]
pub struct WorkspaceRef<'a> {
    pub pool: &'a PgPool,
    pub schema_name: &'a str,
    pub prefix: &'a str,
}

impl<'a> WorkspaceRef<'a> {
    pub fn new(pool: &'a PgPool, schema_name: &'a str, prefix: &'a str) -> Self {
        Self {
            pool,
            schema_name,
            prefix,
        }
    }
}

/// Transport-neutral failure from a service call.
///
/// Deliberately not `AppError`: the library adapter has no notion of an HTTP
/// status. The `From` impl below is the single place the REST status codes and
/// error messages are decided, so both adapters stay consistent with each other
/// and REST's wire format is preserved exactly.
#[derive(Debug, thiserror::Error)]
pub enum ServiceError {
    #[error("Endpoint '{0}' not found")]
    EndpointNotFound(String),

    #[error("Endpoint '{0}' is internal and cannot be used for user-created jobs")]
    InternalEndpoint(String),

    #[error("Job '{0}' not found")]
    JobNotFound(String),

    #[error("{0}")]
    InvalidRequest(String),

    #[error("{0}")]
    InvalidCron(String),

    #[error("Payload spec '{0}' not found")]
    InvalidPayloadSpecRef(String),

    #[error("{0}")]
    InvalidSchema(String),

    #[error("{0}")]
    InputValidationFailed(String),

    #[error("{0}")]
    Conflict(String),

    #[error(transparent)]
    Db(#[from] sqlx::Error),
}

impl From<ServiceError> for AppError {
    fn from(e: ServiceError) -> Self {
        match e {
            ServiceError::EndpointNotFound(n) => AppError::EndpointNotFound(n),
            // Rendered as a 400 with this exact message by the REST path before
            // the service layer existed; kept verbatim.
            ServiceError::InternalEndpoint(n) => AppError::InvalidRequest(format!(
                "Endpoint '{n}' is internal and cannot be used for user-created jobs"
            )),
            ServiceError::JobNotFound(id) => AppError::JobNotFound(id),
            ServiceError::InvalidRequest(m) => AppError::InvalidRequest(m),
            ServiceError::InvalidCron(m) => AppError::InvalidCron(m),
            ServiceError::InvalidPayloadSpecRef(n) => AppError::InvalidPayloadSpecRef(n),
            ServiceError::InvalidSchema(m) => AppError::InvalidSchema(m),
            ServiceError::InputValidationFailed(m) => AppError::InputValidationFailed(m),
            ServiceError::Conflict(m) => AppError::Conflict(m),
            ServiceError::Db(e) => AppError::from(e),
        }
    }
}
