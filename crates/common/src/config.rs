use crate::env::{get_from_env_or_default, get_from_env_unsafe};

// ---------------------------------------------------------------------------
// Sensitive-env reader: transparently KMS-decrypts when the feature is active
// ---------------------------------------------------------------------------

struct SensitiveEnvReader {
    #[cfg(feature = "kms")]
    client: Option<aws_sdk_kms::Client>,
}

impl SensitiveEnvReader {
    /// Read a *required* sensitive env var (KMS-decrypted when enabled).
    async fn read(&self, name: &str) -> Result<String, String> {
        #[cfg(feature = "kms")]
        if let Some(ref client) = self.client {
            return Ok(crate::kms::decrypt(client, name).await);
        }
        get_from_env_unsafe(name)
    }

    /// Read an *optional* sensitive env var, falling back to `default`.
    async fn read_or_default(&self, name: &str, default: String) -> String {
        #[cfg(feature = "kms")]
        if let Some(ref client) = self.client {
            return crate::kms::decrypt_opt(client, name)
                .await
                .unwrap_or(default);
        }
        std::env::var(name).unwrap_or(default)
    }
}

// ---------------------------------------------------------------------------
// Structured env types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct DbEnv {
    pub url: String,
    pub pool_size: u32,
}

impl DbEnv {
    async fn new(reader: &SensitiveEnvReader) -> Result<Self, String> {
        let url = reader.read("TE_DATABASE_URL").await?;
        let pool_size = get_from_env_or_default("TE_DB_POOL_SIZE", 50);
        Ok(Self { url, pool_size })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ServerMode {
    Api,
    Dashboard,
    Both,
}

impl ServerMode {
    fn from_env() -> Self {
        match get_from_env_or_default("TE_MODE", "api".to_string())
            .to_lowercase()
            .as_str()
        {
            "dashboard" => Self::Dashboard,
            "both" => Self::Both,
            _ => Self::Api,
        }
    }
}

fn normalize_prefix(raw: String) -> String {
    let p = raw.trim_matches('/');
    if p.is_empty() {
        String::new()
    } else {
        format!("/{p}")
    }
}

#[derive(Debug, Clone)]
pub struct ServerEnv {
    pub listen_addr: String,
    pub api_key: String,
    pub path_prefix: String,
    pub mode: ServerMode,
    pub dashboard_prefix: String,
    pub dashboard_dist_dir: String,
}

impl ServerEnv {
    async fn new(reader: &SensitiveEnvReader) -> Result<Self, String> {
        let listen_addr =
            get_from_env_or_default("TE_LISTEN_ADDR", "0.0.0.0:8080".to_string());
        let api_key = reader
            .read_or_default("TE_API_KEY", "dev-api-key".to_string())
            .await;
        let path_prefix = normalize_prefix(
            get_from_env_or_default("TE_PATH_PREFIX", String::new()),
        );
        let mode = ServerMode::from_env();
        let dashboard_prefix = normalize_prefix(
            get_from_env_or_default("TE_DASHBOARD_PATH_PREFIX", String::new()),
        );
        let dashboard_dist_dir = get_from_env_or_default(
            "TE_DASHBOARD_DIST_DIR",
            "./dashboard-dist".to_string(),
        );
        Ok(Self {
            listen_addr,
            api_key,
            path_prefix,
            mode,
            dashboard_prefix,
            dashboard_dist_dir,
        })
    }
}

#[derive(Debug, Clone)]
pub struct WorkerEnv {
    pub max_concurrent: usize,
    pub poll_interval_ms: u64,
    pub config_cache_ttl_sec: u64,
    pub secret_cache_ttl_sec: u64,
    pub shutdown_timeout_sec: u64,
}

impl WorkerEnv {
    fn new() -> Self {
        Self {
            max_concurrent: get_from_env_or_default("TE_WORKER_MAX_CONCURRENT", 50),
            poll_interval_ms: get_from_env_or_default("TE_WORKER_POLL_INTERVAL_MS", 200),
            config_cache_ttl_sec: get_from_env_or_default("TE_CONFIG_CACHE_TTL_SEC", 60),
            secret_cache_ttl_sec: get_from_env_or_default("TE_SECRET_CACHE_TTL_SEC", 300),
            shutdown_timeout_sec: get_from_env_or_default(
                "TE_WORKER_SHUTDOWN_TIMEOUT_SEC",
                30,
            ),
        }
    }
}

#[derive(Debug, Clone)]
pub struct CryptoEnv {
    pub encryption_key: String,
}

impl CryptoEnv {
    async fn new(reader: &SensitiveEnvReader) -> Result<Self, String> {
        let encryption_key = reader
            .read_or_default("TE_ENCRYPTION_KEY", "0".repeat(64))
            .await;
        Ok(Self { encryption_key })
    }
}

#[derive(Debug, Clone)]
pub struct MetricsEnv {
    pub port: u16,
}

impl MetricsEnv {
    fn new() -> Self {
        Self {
            port: get_from_env_or_default("TE_METRICS_PORT", 9090),
        }
    }
}

#[derive(Debug, Clone)]
pub struct SchemaEnv {
    pub system_schema: String,
    pub tenant_schema_prefix: String,
}

impl SchemaEnv {
    fn new() -> Self {
        Self {
            system_schema: get_from_env_or_default(
                "TE_SYSTEM_SCHEMA",
                "public".to_string(),
            ),
            tenant_schema_prefix: get_from_env_or_default(
                "TE_TENANT_SCHEMA_PREFIX",
                String::new(),
            ),
        }
    }

    pub fn to_schema_config(&self) -> crate::schema_config::SchemaConfig {
        crate::schema_config::SchemaConfig {
            system_schema: self.system_schema.clone(),
            tenant_schema_prefix: self.tenant_schema_prefix.clone(),
        }
    }
}

// ---------------------------------------------------------------------------
// Top-level config
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub db: DbEnv,
    pub server: ServerEnv,
    pub worker: WorkerEnv,
    pub crypto: CryptoEnv,
    pub metrics: MetricsEnv,
    pub schema: SchemaEnv,
}

impl AppConfig {
    pub async fn from_env() -> anyhow::Result<Self> {
        let kms_enabled: bool = get_from_env_or_default("TE_KMS_ENABLED", false);

        #[cfg(not(feature = "kms"))]
        if kms_enabled {
            anyhow::bail!(
                "TE_KMS_ENABLED=true but kronos was compiled without the 'kms' feature"
            );
        }

        let reader = SensitiveEnvReader {
            #[cfg(feature = "kms")]
            client: if kms_enabled {
                tracing::info!("KMS decryption enabled, initializing AWS KMS client");
                Some(crate::kms::new_client().await)
            } else {
                None
            },
        };

        let db = DbEnv::new(&reader)
            .await
            .map_err(|e| anyhow::anyhow!(e))?;
        let server = ServerEnv::new(&reader)
            .await
            .map_err(|e| anyhow::anyhow!(e))?;
        let worker = WorkerEnv::new();
        let crypto = CryptoEnv::new(&reader)
            .await
            .map_err(|e| anyhow::anyhow!(e))?;
        let metrics = MetricsEnv::new();
        let schema = SchemaEnv::new();

        // Validate schema config early so misconfiguration fails fast at startup.
        schema
            .to_schema_config()
            .validate()
            .map_err(|e| anyhow::anyhow!("invalid schema config: {}", e))?;

        Ok(Self {
            db,
            server,
            worker,
            crypto,
            metrics,
            schema,
        })
    }
}

#[cfg(test)]
mod schema_env_tests {
    use super::*;

    #[test]
    fn defaults_match_today() {
        let saved_sys = std::env::var("TE_SYSTEM_SCHEMA").ok();
        let saved_prefix = std::env::var("TE_TENANT_SCHEMA_PREFIX").ok();
        std::env::remove_var("TE_SYSTEM_SCHEMA");
        std::env::remove_var("TE_TENANT_SCHEMA_PREFIX");

        let s = SchemaEnv::new();
        assert_eq!(s.system_schema, "public");
        assert_eq!(s.tenant_schema_prefix, "");

        if let Some(v) = saved_sys {
            std::env::set_var("TE_SYSTEM_SCHEMA", v);
        }
        if let Some(v) = saved_prefix {
            std::env::set_var("TE_TENANT_SCHEMA_PREFIX", v);
        }
    }

    #[test]
    fn picks_up_non_default_values() {
        std::env::set_var("TE_SYSTEM_SCHEMA", "kronos_test_env");
        std::env::set_var("TE_TENANT_SCHEMA_PREFIX", "k_");

        let s = SchemaEnv::new();
        assert_eq!(s.system_schema, "kronos_test_env");
        assert_eq!(s.tenant_schema_prefix, "k_");

        std::env::remove_var("TE_SYSTEM_SCHEMA");
        std::env::remove_var("TE_TENANT_SCHEMA_PREFIX");
    }
}
