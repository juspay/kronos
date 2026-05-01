use sqlx::PgPool;
use std::fmt;

/// A configured Kronos worker. Construct via [`Worker::builder`] and run with
/// [`Worker::run_until_ctrl_c`] (added in Task 5) or [`Worker::start`] (Task 5).
#[derive(Debug)]
pub struct Worker {
    pub(crate) pool: PgPool,
    pub(crate) cfg: WorkerConfig,
}

/// Internal config built by [`crate::builder::WorkerBuilder::build`]. Holds
/// validated values; intentionally not public — callers shape it via the builder.
#[derive(Clone)]
pub(crate) struct WorkerConfig {
    pub(crate) system_schema: String,
    pub(crate) tenant_schema_prefix: String,
    pub(crate) max_concurrent: usize,
    pub(crate) poll_interval_ms: u64,
    pub(crate) config_cache_ttl_sec: u64,
    pub(crate) secret_cache_ttl_sec: u64,
    pub(crate) shutdown_timeout_sec: u64,
    pub(crate) encryption_key: String,
}

// Manual Debug that redacts `encryption_key` so it never lands in a panic
// message or log line.
impl fmt::Debug for WorkerConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WorkerConfig")
            .field("system_schema", &self.system_schema)
            .field("tenant_schema_prefix", &self.tenant_schema_prefix)
            .field("max_concurrent", &self.max_concurrent)
            .field("poll_interval_ms", &self.poll_interval_ms)
            .field("config_cache_ttl_sec", &self.config_cache_ttl_sec)
            .field("secret_cache_ttl_sec", &self.secret_cache_ttl_sec)
            .field("shutdown_timeout_sec", &self.shutdown_timeout_sec)
            .field("encryption_key", &"<redacted>")
            .finish()
    }
}

impl Worker {
    /// Start a builder for a Worker bound to `pool`.
    pub fn builder(pool: PgPool) -> crate::builder::WorkerBuilder {
        crate::builder::WorkerBuilder::new(pool)
    }
}
