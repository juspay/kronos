//! Builder defaults and `from_app_config` extraction. These tests do NOT touch
//! Postgres — they only exercise pure value construction. Schema-existence
//! validation is covered separately in Task 4.

use kronos_embedded_worker::WorkerBuilder;

// Helper: dummy pool that's never connected. Builder construction must not
// require a live DB.
fn dummy_pool() -> sqlx::PgPool {
    sqlx::PgPool::connect_lazy("postgres://example:example@127.0.0.1:1/none")
        .expect("connect_lazy should not error on a syntactically valid url")
}

#[tokio::test]
async fn library_defaults_use_kronos_namespace() {
    let b = WorkerBuilder::new(dummy_pool());
    assert_eq!(b.system_schema_for_test(), "kronos");
    assert_eq!(b.tenant_schema_prefix_for_test(), "kronos_");
    assert_eq!(b.max_concurrent_for_test(), 50);
    assert_eq!(b.poll_interval_ms_for_test(), 200);
    assert_eq!(b.config_cache_ttl_sec_for_test(), 60);
    assert_eq!(b.secret_cache_ttl_sec_for_test(), 300);
    assert_eq!(b.shutdown_timeout_sec_for_test(), 30);
}

#[tokio::test]
async fn setters_override_defaults() {
    let b = WorkerBuilder::new(dummy_pool())
        .system_schema("acme".into())
        .tenant_schema_prefix("acme_".into())
        .max_concurrent(7)
        .poll_interval_ms(1234)
        .config_cache_ttl_sec(11)
        .secret_cache_ttl_sec(22)
        .shutdown_timeout_sec(33)
        .encryption_key("0".repeat(64));

    assert_eq!(b.system_schema_for_test(), "acme");
    assert_eq!(b.tenant_schema_prefix_for_test(), "acme_");
    assert_eq!(b.max_concurrent_for_test(), 7);
    assert_eq!(b.poll_interval_ms_for_test(), 1234);
    assert_eq!(b.config_cache_ttl_sec_for_test(), 11);
    assert_eq!(b.secret_cache_ttl_sec_for_test(), 22);
    assert_eq!(b.shutdown_timeout_sec_for_test(), 33);
}

#[tokio::test]
async fn from_app_config_pulls_through_service_defaults() {
    // Drive the binary's env-derived path with the canonical service defaults.
    std::env::set_var("TE_DATABASE_URL", "postgres://e:e@127.0.0.1:1/none");
    std::env::remove_var("TE_SYSTEM_SCHEMA");
    std::env::remove_var("TE_TENANT_SCHEMA_PREFIX");
    std::env::set_var("TE_ENCRYPTION_KEY", "0".repeat(64));
    let cfg = kronos_common::config::AppConfig::from_env()
        .await
        .expect("AppConfig::from_env should succeed with a syntactically valid TE_DATABASE_URL");

    let b = WorkerBuilder::new(dummy_pool()).from_app_config(&cfg);
    assert_eq!(b.system_schema_for_test(), "public");
    assert_eq!(b.tenant_schema_prefix_for_test(), "");
    assert_eq!(b.max_concurrent_for_test(), 50);
    assert_eq!(b.poll_interval_ms_for_test(), 200);

    std::env::remove_var("TE_DATABASE_URL");
    std::env::remove_var("TE_ENCRYPTION_KEY");
}

// The tests below need a running Postgres at TE_DATABASE_URL.
// Run with: `cargo test -p kronos-embedded-worker --test builder -- --ignored --test-threads=1`
use kronos_embedded_worker::BuildError;

fn db_url() -> String {
    std::env::var("TE_DATABASE_URL").unwrap_or_else(|_| {
        "postgres://kronos:kronos@localhost:5432/taskexecutor".to_string()
    })
}

#[tokio::test]
#[ignore]
async fn build_rejects_invalid_schema_config() {
    let pool = sqlx::PgPool::connect(&db_url()).await.unwrap();
    let err = kronos_embedded_worker::Worker::builder(pool)
        .system_schema("public; DROP TABLE x".into())
        .encryption_key("0".repeat(64))
        .build()
        .await
        .expect_err("build must reject SQL-injection-style schema names");
    assert!(matches!(err, BuildError::InvalidSchemaConfig(_)));
}

#[tokio::test]
#[ignore]
async fn build_rejects_missing_system_schema() {
    let pool = sqlx::PgPool::connect(&db_url()).await.unwrap();
    let err = kronos_embedded_worker::Worker::builder(pool)
        .system_schema("kronos_does_not_exist_42".into())
        .encryption_key("0".repeat(64))
        .build()
        .await
        .expect_err("build must reject when system schema is missing");
    match err {
        BuildError::SystemSchemaMissing { schema, table } => {
            assert_eq!(schema, "kronos_does_not_exist_42");
            assert_eq!(table, "organizations");
        }
        other => panic!("expected SystemSchemaMissing, got {other:?}"),
    }
}

#[tokio::test]
#[ignore]
async fn build_succeeds_with_default_public_schema() {
    let pool = sqlx::PgPool::connect(&db_url()).await.unwrap();
    let _worker = kronos_embedded_worker::Worker::builder(pool)
        .system_schema("public".into())
        .tenant_schema_prefix("".into())
        .encryption_key("0".repeat(64))
        .build()
        .await
        .expect("build should succeed against a migrated public schema");
}

#[tokio::test]
#[ignore]
async fn build_rejects_missing_encryption_key() {
    let pool = sqlx::PgPool::connect(&db_url()).await.unwrap();
    let err = kronos_embedded_worker::Worker::builder(pool)
        .system_schema("public".into())
        .tenant_schema_prefix("".into())
        .build()
        .await
        .expect_err("build must reject when encryption_key is unset");
    assert!(matches!(err, BuildError::EncryptionKeyMissing));
}
