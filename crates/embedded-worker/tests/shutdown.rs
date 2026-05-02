//! Verifies `Worker::start()` + `WorkerHandle::shutdown()` drains and returns
//! Ok(()) under a normal start/stop cycle. Requires a migrated DB.
//! Run with: `cargo test -p kronos-embedded-worker --test shutdown -- --ignored --test-threads=1`

use std::time::Duration;

fn db_url() -> String {
    std::env::var("TE_DATABASE_URL").unwrap_or_else(|_| {
        "postgres://kronos:kronos@localhost:5432/taskexecutor".to_string()
    })
}

#[tokio::test]
#[ignore]
async fn start_then_shutdown_returns_clean() {
    let pool = sqlx::PgPool::connect(&db_url()).await.unwrap();
    let worker = kronos_embedded_worker::Worker::builder(pool)
        .system_schema("public".into())
        .tenant_schema_prefix("".into())
        .encryption_key("0".repeat(64))
        .build()
        .await
        .expect("build against migrated public schema");

    let handle = worker.start();
    // Let the loop spin a few times so we exercise the shutdown branch from
    // an active poll cycle, not just the first iteration.
    tokio::time::sleep(Duration::from_millis(500)).await;
    handle.shutdown().await.expect("graceful shutdown returns Ok");
}
