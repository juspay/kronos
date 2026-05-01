use kronos_common::config::AppConfig;
use kronos_common::schema_config::SchemaConfig;
use sqlx::PgPool;

use crate::error::BuildError;
use crate::worker::{Worker, WorkerConfig};

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

impl WorkerBuilder {
    /// Validate the config, probe the system schema, and produce a [`Worker`].
    /// When `install_metrics_recorder(true)` was called, the metrics recorder
    /// is installed on `metrics_port` exactly once before returning.
    pub async fn build(self) -> Result<Worker, BuildError> {
        // 1. Schema-name shape validation (no DB call).
        let cfg = SchemaConfig {
            system_schema: self.system_schema.clone(),
            tenant_schema_prefix: self.tenant_schema_prefix.clone(),
        };
        cfg.validate().map_err(BuildError::InvalidSchemaConfig)?;

        // 2. Encryption key required for v1.
        let encryption_key = self
            .encryption_key
            .clone()
            .ok_or(BuildError::EncryptionKeyMissing)?;

        // 3. System-schema existence probe via to_regclass (null-safe; no parse error
        //    when schema or table is missing). system_schema is already shape-validated,
        //    so quoting it is safe.
        let qualified_orgs = format!("\"{}\".organizations", self.system_schema);
        let qualified_ws = format!("\"{}\".workspaces", self.system_schema);
        let probe: (Option<String>, Option<String>) = sqlx::query_as(
            "SELECT to_regclass($1)::text, to_regclass($2)::text",
        )
        .bind(&qualified_orgs)
        .bind(&qualified_ws)
        .fetch_one(&self.pool)
        .await?;

        if probe.0.is_none() {
            return Err(BuildError::SystemSchemaMissing {
                schema: self.system_schema.clone(),
                table: "organizations".into(),
            });
        }
        if probe.1.is_none() {
            return Err(BuildError::SystemSchemaMissing {
                schema: self.system_schema.clone(),
                table: "workspaces".into(),
            });
        }

        // 4. Optional metrics recorder install — service-binary opt-in.
        if self.install_metrics_recorder {
            kronos_common::metrics::install_recorder_with_listener(self.metrics_port);
        }

        Ok(Worker {
            pool: self.pool,
            cfg: WorkerConfig {
                system_schema: self.system_schema,
                tenant_schema_prefix: self.tenant_schema_prefix,
                max_concurrent: self.max_concurrent,
                poll_interval_ms: self.poll_interval_ms,
                config_cache_ttl_sec: self.config_cache_ttl_sec,
                secret_cache_ttl_sec: self.secret_cache_ttl_sec,
                shutdown_timeout_sec: self.shutdown_timeout_sec,
                encryption_key,
            },
        })
    }
}
