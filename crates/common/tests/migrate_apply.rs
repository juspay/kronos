//! Integration test: `migrations::apply` produces a working schema for
//! both service-default and library-default `SchemaConfig`s.
//!
//! Requires a running Postgres at `TE_DATABASE_URL` (or `postgres://kronos:kronos@localhost:5432/taskexecutor`).
//! Run with: `cargo test -p kronos-common --test migrate_apply -- --test-threads=1`

use kronos_common::migrations;
use kronos_common::schema_config::SchemaConfig;
use sqlx::PgPool;

fn db_url() -> String {
    std::env::var("TE_DATABASE_URL").unwrap_or_else(|_| {
        "postgres://kronos:kronos@localhost:5432/taskexecutor".to_string()
    })
}

async fn fresh_pool() -> PgPool {
    let pool = PgPool::connect(&db_url()).await.unwrap();
    // Reset both candidate system schemas so the test is reentrant.
    for schema in &["kronos_test", "public"] {
        let _ = sqlx::query(&format!(
            "DROP SCHEMA IF EXISTS \"{}\" CASCADE; CREATE SCHEMA \"{}\";",
            schema, schema
        ))
        .execute(&pool)
        .await;
    }
    pool
}

#[tokio::test]
#[ignore] // requires DB; run explicitly
async fn applies_with_service_default() {
    let pool = fresh_pool().await;
    let cfg = SchemaConfig::service_default();
    migrations::apply(&pool, &cfg).await.unwrap();

    // Sanity: organizations and workspaces exist in `public`.
    let exists: (bool,) = sqlx::query_as(
        "SELECT EXISTS (SELECT 1 FROM information_schema.tables \
         WHERE table_schema = 'public' AND table_name = 'organizations')",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(exists.0);
}

#[tokio::test]
#[ignore]
async fn applies_with_library_default() {
    let pool = fresh_pool().await;
    let cfg = SchemaConfig {
        system_schema: "kronos_test".to_string(),
        tenant_schema_prefix: "kronos_".to_string(),
    };
    migrations::apply(&pool, &cfg).await.unwrap();

    let exists: (bool,) = sqlx::query_as(
        "SELECT EXISTS (SELECT 1 FROM information_schema.tables \
         WHERE table_schema = 'kronos_test' AND table_name = 'organizations')",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(exists.0);
}
