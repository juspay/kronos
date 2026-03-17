use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct AppConfig {
    #[serde(default = "default_listen_addr")]
    pub listen_addr: String,
    #[serde(alias = "TE_DATABASE_URL")]
    pub database_url: String,
    #[serde(default = "default_db_pool_size")]
    pub db_pool_size: u32,
    #[serde(default = "default_api_key")]
    pub api_key: String,
    #[serde(default = "default_encryption_key")]
    pub encryption_key: String,

    // Worker
    #[serde(default = "default_max_concurrent")]
    pub worker_max_concurrent: usize,
    #[serde(default = "default_poll_interval")]
    pub worker_poll_interval_ms: u64,
    #[serde(default = "default_config_cache_ttl")]
    pub config_cache_ttl_sec: u64,
    #[serde(default = "default_secret_cache_ttl")]
    pub secret_cache_ttl_sec: u64,
    #[serde(default = "default_shutdown_timeout")]
    pub worker_shutdown_timeout_sec: u64,

    // Scheduler
    #[serde(default = "default_cron_tick_interval")]
    pub cron_tick_interval_sec: u64,
    #[serde(default = "default_cron_batch_size")]
    pub cron_batch_size: i64,
    #[serde(default = "default_promote_interval")]
    pub promote_interval_ms: u64,
    #[serde(default = "default_reclaim_interval")]
    pub reclaim_interval_sec: u64,
    #[serde(default = "default_stuck_timeout")]
    pub stuck_execution_timeout_sec: i64,
}

fn default_listen_addr() -> String { "0.0.0.0:8080".into() }
fn default_db_pool_size() -> u32 { 20 }
fn default_api_key() -> String { "dev-api-key".into() }
fn default_encryption_key() -> String { "0".repeat(64) }
fn default_max_concurrent() -> usize { 50 }
fn default_poll_interval() -> u64 { 200 }
fn default_config_cache_ttl() -> u64 { 60 }
fn default_secret_cache_ttl() -> u64 { 300 }
fn default_shutdown_timeout() -> u64 { 30 }
fn default_cron_tick_interval() -> u64 { 1 }
fn default_cron_batch_size() -> i64 { 100 }
fn default_promote_interval() -> u64 { 500 }
fn default_reclaim_interval() -> u64 { 30 }
fn default_stuck_timeout() -> i64 { 300 }

impl AppConfig {
    pub fn from_env() -> anyhow::Result<Self> {
        // Map TE_ prefixed env vars to struct fields
        let config = Self {
            listen_addr: std::env::var("TE_LISTEN_ADDR").unwrap_or_else(|_| default_listen_addr()),
            database_url: std::env::var("TE_DATABASE_URL")
                .expect("TE_DATABASE_URL must be set"),
            db_pool_size: std::env::var("TE_DB_POOL_SIZE")
                .ok().and_then(|v| v.parse().ok()).unwrap_or_else(default_db_pool_size),
            api_key: std::env::var("TE_API_KEY").unwrap_or_else(|_| default_api_key()),
            encryption_key: std::env::var("TE_ENCRYPTION_KEY").unwrap_or_else(|_| default_encryption_key()),
            worker_max_concurrent: std::env::var("TE_WORKER_MAX_CONCURRENT")
                .ok().and_then(|v| v.parse().ok()).unwrap_or_else(default_max_concurrent),
            worker_poll_interval_ms: std::env::var("TE_WORKER_POLL_INTERVAL_MS")
                .ok().and_then(|v| v.parse().ok()).unwrap_or_else(default_poll_interval),
            config_cache_ttl_sec: std::env::var("TE_CONFIG_CACHE_TTL_SEC")
                .ok().and_then(|v| v.parse().ok()).unwrap_or_else(default_config_cache_ttl),
            secret_cache_ttl_sec: std::env::var("TE_SECRET_CACHE_TTL_SEC")
                .ok().and_then(|v| v.parse().ok()).unwrap_or_else(default_secret_cache_ttl),
            worker_shutdown_timeout_sec: std::env::var("TE_WORKER_SHUTDOWN_TIMEOUT_SEC")
                .ok().and_then(|v| v.parse().ok()).unwrap_or_else(default_shutdown_timeout),
            cron_tick_interval_sec: std::env::var("TE_CRON_TICK_INTERVAL_SEC")
                .ok().and_then(|v| v.parse().ok()).unwrap_or_else(default_cron_tick_interval),
            cron_batch_size: std::env::var("TE_CRON_BATCH_SIZE")
                .ok().and_then(|v| v.parse().ok()).unwrap_or_else(default_cron_batch_size),
            promote_interval_ms: std::env::var("TE_PROMOTE_INTERVAL_MS")
                .ok().and_then(|v| v.parse().ok()).unwrap_or_else(default_promote_interval),
            reclaim_interval_sec: std::env::var("TE_RECLAIM_INTERVAL_SEC")
                .ok().and_then(|v| v.parse().ok()).unwrap_or_else(default_reclaim_interval),
            stuck_execution_timeout_sec: std::env::var("TE_STUCK_EXECUTION_TIMEOUT_SEC")
                .ok().and_then(|v| v.parse().ok()).unwrap_or_else(default_stuck_timeout),
        };
        Ok(config)
    }
}
