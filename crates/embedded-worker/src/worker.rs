use sqlx::PgPool;
use std::fmt;

use crate::handle::WorkerHandle;
use tokio::sync::oneshot;

/// A configured Kronos worker. Construct via [`Worker::builder`] and run with
/// [`Worker::run_until_ctrl_c`] or [`Worker::start`].
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

    /// Run the worker loop until SIGINT (Ctrl-C). Service-binary convenience.
    /// Embedded hosts that need their own shutdown story should use [`Worker::start`].
    pub async fn run_until_ctrl_c(self) -> anyhow::Result<()> {
        let shutdown = async {
            let _ = tokio::signal::ctrl_c().await;
        };
        crate::poller::run_loop(self.pool, self.cfg, shutdown).await
    }

    /// Spawn the worker loop on the current Tokio runtime and return a handle.
    /// The handle's `shutdown()` triggers a graceful drain bounded by
    /// `shutdown_timeout_sec`.
    ///
    /// # Panics
    ///
    /// Panics if called outside the context of a Tokio runtime, because it
    /// uses [`tokio::spawn`] internally.
    pub fn start(self) -> WorkerHandle {
        let (tx, rx) = oneshot::channel::<()>();
        let join = tokio::spawn(async move {
            let shutdown = async move {
                let _ = rx.await;
            };
            crate::poller::run_loop(self.pool, self.cfg, shutdown).await
        });
        WorkerHandle {
            shutdown_tx: Some(tx),
            join: Some(join),
        }
    }
}
