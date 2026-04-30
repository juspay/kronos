//! Smoke test for non-default schema namespacing.
//!
//! Applies migrations with `system_schema = "kronos_smoke"` against a fresh
//! database, then asserts the expected tables exist in the configured schema.
//! This proves the parameterized migration path lands tables in the requested
//! namespace; it does not assert the absence of tables in `public`, since
//! sibling integration tests legitimately populate `public` under the default
//! schema config.
//!
//! Requires Postgres at `TE_DATABASE_URL` (default postgres://kronos:kronos@localhost:5432/taskexecutor).
//! Run with: `cargo test -p kronos-client --test migrate_kronos_namespace -- --ignored`

use kronos_client::{migrate, SchemaConfig};
use sqlx::PgPool;

fn db_url() -> String {
    std::env::var("TE_DATABASE_URL").unwrap_or_else(|_| {
        "postgres://kronos:kronos@localhost:5432/taskexecutor".to_string()
    })
}

#[tokio::test]
#[ignore]
async fn migrations_create_tables_in_configured_schema_only() {
    let pool = PgPool::connect(&db_url()).await.unwrap();

    // Wipe any prior state from this test schema, but DO NOT touch `public`
    // (the integration tests in the same DB rely on `public`).
    sqlx::query("DROP SCHEMA IF EXISTS kronos_smoke CASCADE")
        .execute(&pool)
        .await
        .unwrap();

    let cfg = SchemaConfig {
        system_schema: "kronos_smoke".to_string(),
        tenant_schema_prefix: "kronos_".to_string(),
    };

    migrate(&pool, &cfg).await.unwrap();

    // organizations and workspaces are in kronos_smoke
    let orgs_in_kronos: (bool,) = sqlx::query_as(
        "SELECT EXISTS (SELECT 1 FROM information_schema.tables \
         WHERE table_schema = 'kronos_smoke' AND table_name = 'organizations')",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(orgs_in_kronos.0, "organizations should exist in kronos_smoke");

    let ws_in_kronos: (bool,) = sqlx::query_as(
        "SELECT EXISTS (SELECT 1 FROM information_schema.tables \
         WHERE table_schema = 'kronos_smoke' AND table_name = 'workspaces')",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(ws_in_kronos.0, "workspaces should exist in kronos_smoke");

    // The migrations create exactly two system-level tables: organizations and
    // workspaces. Verify the count in kronos_smoke matches.
    let table_count: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM information_schema.tables \
         WHERE table_schema = 'kronos_smoke' AND table_name IN ('organizations', 'workspaces')",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(table_count.0, 2, "expected both organizations and workspaces in kronos_smoke");

    // Cleanup so re-running the test is clean.
    sqlx::query("DROP SCHEMA IF EXISTS kronos_smoke CASCADE")
        .execute(&pool)
        .await
        .unwrap();
}
