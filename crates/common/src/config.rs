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

#[derive(Debug, Clone)]
pub struct ServerEnv {
    pub listen_addr: String,
    pub api_key: String,
    pub path_prefix: String,
}

impl ServerEnv {
    async fn new(reader: &SensitiveEnvReader) -> Result<Self, String> {
        let listen_addr =
            get_from_env_or_default("TE_LISTEN_ADDR", "0.0.0.0:8080".to_string());
        let api_key = reader
            .read_or_default("TE_API_KEY", "dev-api-key".to_string())
            .await;
        let path_prefix =
            get_from_env_or_default("TE_PATH_PREFIX", String::new());
        // Normalize: ensure it starts with '/' and has no trailing '/'.
        // A bare "/" or empty string both map to "" (no prefix).
        let path_prefix = {
            let p = path_prefix.trim_matches('/');
            if p.is_empty() {
                String::new()
            } else {
                format!("/{p}")
            }
        };
        Ok(Self {
            listen_addr,
            api_key,
            path_prefix,
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

        Ok(Self {
            db,
            server,
            worker,
            crypto,
            metrics,
        })
    }
}
