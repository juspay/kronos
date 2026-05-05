use chrono::{DateTime, Utc};
use kronos_common::{
    cache::{ConfigCache, SecretCache},
    db,
    tenant::{SchemaProvider, validate_table_prefix},
};
use reqwest::Client;
use sqlx::PgPool;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

use crate::pipeline::PipelineContext;
use crate::poller;

/// How a job should be triggered.
pub enum JobTrigger {
    /// Fire immediately, create a QUEUED execution right away.
    Immediate,
    /// Fire at a specific future time.
    Delayed { run_at: DateTime<Utc> },
    /// Recurring CRON schedule.
    Cron {
        expression: String,
        timezone: String,
        starts_at: Option<DateTime<Utc>>,
        ends_at: Option<DateTime<Utc>>,
        first_run_at: DateTime<Utc>,
    },
}

/// Configuration for the background worker.
pub struct WorkerConfig {
    pub max_concurrent: usize,
    pub poll_interval_ms: u64,
    pub config_cache_ttl_sec: u64,
    pub secret_cache_ttl_sec: u64,
    pub shutdown_timeout_sec: u64,
}

impl Default for WorkerConfig {
    fn default() -> Self {
        Self {
            max_concurrent: 50,
            poll_interval_ms: 200,
            config_cache_ttl_sec: 60,
            secret_cache_ttl_sec: 300,
            shutdown_timeout_sec: 30,
        }
    }
}

/// The public API for embedding Kronos in another application.
///
/// Holds a caller-provided `PgPool` and exposes job creation, endpoint
/// registration, and worker startup. The caller controls pool sizing.
#[derive(Clone)]
pub struct KronosClient {
    pool: PgPool,
    ctx: Arc<PipelineContext>,
}

impl KronosClient {
    /// Create a new client.
    ///
    /// - `pool`: caller-owned sqlx pool pointing at the same PostgreSQL instance.
    /// - `table_prefix`: prefix for all Kronos tables (e.g. `"sched"` → `sched_jobs`).
    ///   Empty string means no prefix (original table names).
    /// - `encryption_key`: 64 hex-char AES-256 key for secrets; pass zeros if not using secrets.
    /// - `http_client`: optional reqwest client to reuse the caller's connection pool.
    pub fn new(
        pool: PgPool,
        table_prefix: &str,
        encryption_key: &str,
        http_client: Option<Client>,
    ) -> anyhow::Result<Self> {
        if !validate_table_prefix(table_prefix) {
            anyhow::bail!(
                "table_prefix '{}' is invalid: only alphanumeric and underscore allowed",
                table_prefix
            );
        }

        let ctx = Arc::new(PipelineContext {
            pool: pool.clone(),
            http_client: http_client.unwrap_or_default(),
            config_cache: ConfigCache::new(60),
            secret_cache: SecretCache::new(300),
            encryption_key: encryption_key.to_string(),
            table_prefix: table_prefix.to_string(),
        });

        Ok(Self { pool, ctx })
    }

    /// Create a job in the given workspace schema and return the execution_id.
    pub async fn create_job(
        &self,
        schema_name: &str,
        endpoint: &str,
        input: serde_json::Value,
        max_attempts: i64,
        trigger: JobTrigger,
        idempotency_key: Option<&str>,
    ) -> anyhow::Result<String> {
        let prefix = self.ctx.table_prefix.as_str();
        let ikey = idempotency_key.unwrap_or("");

        let mut conn = db::scoped::scoped_connection(&self.pool, schema_name).await?;

        // First look up the endpoint to get its type
        let ep = db::endpoints::get(&mut *conn, prefix, endpoint)
            .await?
            .ok_or_else(|| anyhow::anyhow!("Endpoint '{}' not found in schema '{}'", endpoint, schema_name))?;

        let execution_id = match trigger {
            JobTrigger::Immediate => {
                let result = db::jobs::create_immediate(
                    &mut *conn,
                    prefix,
                    endpoint,
                    ep.endpoint_type.as_str(),
                    ikey,
                    Some(&input),
                    max_attempts,
                )
                .await?;
                result.execution_id
            }
            JobTrigger::Delayed { run_at } => {
                let result = db::jobs::create_delayed(
                    &mut *conn,
                    prefix,
                    endpoint,
                    ep.endpoint_type.as_str(),
                    ikey,
                    Some(&input),
                    run_at,
                    max_attempts,
                )
                .await?;
                result.execution_id
            }
            JobTrigger::Cron {
                expression,
                timezone,
                starts_at,
                ends_at,
                first_run_at,
            } => {
                let job = db::jobs::create_cron(
                    &mut *conn,
                    prefix,
                    endpoint,
                    ep.endpoint_type.as_str(),
                    Some(&input),
                    &expression,
                    &timezone,
                    starts_at,
                    ends_at,
                    first_run_at,
                )
                .await?;
                job.job_id
            }
        };

        Ok(execution_id)
    }

    /// Register (upsert) an endpoint in the given workspace schema.
    pub async fn register_endpoint(
        &self,
        schema_name: &str,
        name: &str,
        endpoint_type: &str,
        spec: serde_json::Value,
        retry_policy: Option<serde_json::Value>,
    ) -> anyhow::Result<()> {
        let prefix = self.ctx.table_prefix.as_str();
        let mut conn = db::scoped::scoped_connection(&self.pool, schema_name).await?;

        // Upsert: try insert, if conflict update spec and retry_policy
        let existing = db::endpoints::get(&mut *conn, prefix, name).await?;
        if existing.is_none() {
            db::endpoints::create(
                &mut *conn,
                prefix,
                name,
                endpoint_type,
                None,
                None,
                &spec,
                retry_policy.as_ref(),
            )
            .await?;
        } else {
            db::endpoints::update(
                &mut *conn,
                prefix,
                name,
                Some(&spec),
                None,
                None,
                retry_policy.as_ref(),
            )
            .await?;
        }

        Ok(())
    }

    /// Delete an endpoint from the given workspace schema.
    pub async fn delete_endpoint(
        &self,
        schema_name: &str,
        name: &str,
    ) -> anyhow::Result<()> {
        let prefix = self.ctx.table_prefix.as_str();
        let mut conn = db::scoped::scoped_connection(&self.pool, schema_name).await?;
        db::endpoints::delete(&mut *conn, prefix, name).await?;
        Ok(())
    }

    /// Start the background worker. Returns a JoinHandle — the caller should
    /// await it (or drop it) on shutdown.
    ///
    /// Pass a `WorkerConfig` to control concurrency, poll interval, etc.
    /// Pass a `CancellationToken` that the caller cancels on shutdown.
    pub fn start_worker<S: SchemaProvider>(
        &self,
        schema_provider: S,
        cancel: CancellationToken,
        worker_config: WorkerConfig,
    ) -> tokio::task::JoinHandle<anyhow::Result<()>> {
        let pool = self.pool.clone();
        let ctx = self.ctx.clone();

        // Build an AppConfig-compatible struct from the context + worker_config
        let config = build_app_config(&ctx, &worker_config);

        tokio::spawn(async move {
            poller::run(pool, config, schema_provider, cancel).await
        })
    }
}

/// Build an AppConfig from the PipelineContext and WorkerConfig for the poller.
fn build_app_config(ctx: &PipelineContext, wc: &WorkerConfig) -> kronos_common::config::AppConfig {
    use kronos_common::config::{
        AppConfig, CryptoEnv, DbEnv, MetricsEnv, ServerEnv, ServerMode, WorkerEnv,
    };

    AppConfig {
        db: DbEnv {
            url: String::new(), // pool already created by caller
            pool_size: 0,
            table_prefix: ctx.table_prefix.clone(),
        },
        server: ServerEnv {
            listen_addr: String::new(),
            api_key: String::new(),
            path_prefix: String::new(),
            mode: ServerMode::Api,
            dashboard_prefix: String::new(),
            dashboard_dist_dir: String::new(),
        },
        worker: WorkerEnv {
            max_concurrent: wc.max_concurrent,
            poll_interval_ms: wc.poll_interval_ms,
            config_cache_ttl_sec: wc.config_cache_ttl_sec,
            secret_cache_ttl_sec: wc.secret_cache_ttl_sec,
            shutdown_timeout_sec: wc.shutdown_timeout_sec,
        },
        crypto: CryptoEnv {
            encryption_key: ctx.encryption_key.clone(),
        },
        metrics: MetricsEnv { port: 0 },
    }
}
