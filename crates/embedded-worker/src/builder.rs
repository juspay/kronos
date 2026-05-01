use kronos_common::config::AppConfig;
use kronos_common::schema_config::SchemaConfig;
use sqlx::PgPool;

/// Builder for a [`Worker`]. See `Worker::builder`.
pub struct WorkerBuilder {
    pub(crate) pool: PgPool,
    pub(crate) system_schema: String,
    pub(crate) tenant_schema_prefix: String,
    pub(crate) max_concurrent: usize,
    pub(crate) poll_interval_ms: u64,
    pub(crate) config_cache_ttl_sec: u64,
    pub(crate) secret_cache_ttl_sec: u64,
    pub(crate) shutdown_timeout_sec: u64,
    pub(crate) encryption_key: Option<String>,
    pub(crate) install_metrics_recorder: bool,
    pub(crate) metrics_port: u16,
}

impl WorkerBuilder {
    pub fn new(pool: PgPool) -> Self {
        let defaults = SchemaConfig::library_default();
        Self {
            pool,
            system_schema: defaults.system_schema,
            tenant_schema_prefix: defaults.tenant_schema_prefix,
            max_concurrent: 50,
            poll_interval_ms: 200,
            config_cache_ttl_sec: 60,
            secret_cache_ttl_sec: 300,
            shutdown_timeout_sec: 30,
            encryption_key: None,
            install_metrics_recorder: false,
            metrics_port: 9090,
        }
    }

    pub fn system_schema(mut self, v: String) -> Self {
        self.system_schema = v;
        self
    }
    pub fn tenant_schema_prefix(mut self, v: String) -> Self {
        self.tenant_schema_prefix = v;
        self
    }
    pub fn max_concurrent(mut self, v: usize) -> Self {
        self.max_concurrent = v;
        self
    }
    pub fn poll_interval_ms(mut self, v: u64) -> Self {
        self.poll_interval_ms = v;
        self
    }
    pub fn config_cache_ttl_sec(mut self, v: u64) -> Self {
        self.config_cache_ttl_sec = v;
        self
    }
    pub fn secret_cache_ttl_sec(mut self, v: u64) -> Self {
        self.secret_cache_ttl_sec = v;
        self
    }
    pub fn shutdown_timeout_sec(mut self, v: u64) -> Self {
        self.shutdown_timeout_sec = v;
        self
    }
    pub fn encryption_key(mut self, v: String) -> Self {
        self.encryption_key = Some(v);
        self
    }
    pub fn install_metrics_recorder(mut self, v: bool) -> Self {
        self.install_metrics_recorder = v;
        self
    }
    pub fn metrics_port(mut self, v: u16) -> Self {
        self.metrics_port = v;
        self
    }

    /// Adapter that copies env-derived config into the builder. Used by the
    /// `kronos-worker` binary to preserve service-mode defaults
    /// (`system_schema = "public"`, `tenant_schema_prefix = ""`).
    pub fn from_app_config(mut self, cfg: &AppConfig) -> Self {
        self.system_schema = cfg.schema.system_schema.clone();
        self.tenant_schema_prefix = cfg.schema.tenant_schema_prefix.clone();
        self.max_concurrent = cfg.worker.max_concurrent;
        self.poll_interval_ms = cfg.worker.poll_interval_ms;
        self.config_cache_ttl_sec = cfg.worker.config_cache_ttl_sec;
        self.secret_cache_ttl_sec = cfg.worker.secret_cache_ttl_sec;
        self.shutdown_timeout_sec = cfg.worker.shutdown_timeout_sec;
        self.encryption_key = Some(cfg.crypto.encryption_key.clone());
        self.metrics_port = cfg.metrics.port;
        self
    }

    // Test-only accessors so unit tests don't have to go through `build()`.
    #[doc(hidden)]
    pub fn system_schema_for_test(&self) -> &str { &self.system_schema }
    #[doc(hidden)]
    pub fn tenant_schema_prefix_for_test(&self) -> &str { &self.tenant_schema_prefix }
    #[doc(hidden)]
    pub fn max_concurrent_for_test(&self) -> usize { self.max_concurrent }
    #[doc(hidden)]
    pub fn poll_interval_ms_for_test(&self) -> u64 { self.poll_interval_ms }
    #[doc(hidden)]
    pub fn config_cache_ttl_sec_for_test(&self) -> u64 { self.config_cache_ttl_sec }
    #[doc(hidden)]
    pub fn secret_cache_ttl_sec_for_test(&self) -> u64 { self.secret_cache_ttl_sec }
    #[doc(hidden)]
    pub fn shutdown_timeout_sec_for_test(&self) -> u64 { self.shutdown_timeout_sec }
}
