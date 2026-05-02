//! Temporary shim for Plan 2. Deleted in Task 6.
use kronos_common::config::AppConfig;
use sqlx::PgPool;

pub mod poller {
    use super::*;
    pub async fn run(pool: PgPool, config: AppConfig) -> anyhow::Result<()> {
        kronos_embedded_worker::Worker::builder(pool)
            .from_app_config(&config)
            .build()
            .await
            .map_err(|e| anyhow::anyhow!(e))?
            .run_until_ctrl_c()
            .await
    }
}
