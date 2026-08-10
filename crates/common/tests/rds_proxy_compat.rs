//! Guards the invariants that let kronos run behind a transaction-pooling
//! Postgres proxy (AWS RDS Proxy, PgBouncer in transaction mode).
//!
//! Such a proxy multiplexes many client sessions onto few backend connections,
//! swapping them between transactions. It can only do that while a session
//! carries no state of its own. The moment a client mutates session state the
//! proxy must *pin* it to one backend connection for the rest of its life,
//! which collapses the proxy into a plain TCP passthrough and can exhaust
//! `max_connections` faster than having no proxy at all.
//!
//! Kronos used to set `search_path` per workspace on checkout, which pinned
//! every connection it ever opened. Tenant scope now travels in the SQL text
//! (`db::tbl`), and the sqlx statement cache is disabled (`db::connect_pool`),
//! so nothing kronos runs leaves state behind. These tests assert that, and
//! assert the tenant isolation that the qualified names are responsible for.
//!
//! Requires a live PostgreSQL with kronos's `public` control tables. Point
//! `KRONOS_TEST_DATABASE_URL` at a scratch database; without it these tests
//! skip rather than fail, so `cargo test` stays green on a machine with no DB.

use kronos_common::db::{self, DbContext};
use sqlx::{PgPool, Row};

const DB_URL_ENV: &str = "KRONOS_TEST_DATABASE_URL";

/// PostgreSQL's out-of-the-box `search_path`. Any deviation on a pooled
/// connection means someone issued a `SET` and pinned it.
const DEFAULT_SEARCH_PATH: &str = "\"$user\", public";

/// Connect, or return `None` so the test self-skips on a machine with no DB.
async fn pool() -> Option<PgPool> {
    let url = std::env::var(DB_URL_ENV).ok()?;
    Some(
        db::connect_pool(&url, 5)
            .await
            .unwrap_or_else(|e| panic!("could not connect to {DB_URL_ENV}: {e}")),
    )
}

/// A throwaway schema name, unique per test run so parallel tests never collide.
fn unique_schema(tag: &str) -> String {
    format!("t_{}_{}", tag, uuid::Uuid::new_v4().simple())
}

async fn drop_schema(pool: &PgPool, schema: &str) {
    sqlx::query(&format!("DROP SCHEMA IF EXISTS \"{schema}\" CASCADE"))
        .execute(pool)
        .await
        .expect("cleanup: drop schema");
}

/// Read `search_path` as the server currently sees it on this connection.
async fn search_path_of(conn: &mut sqlx::PgConnection) -> String {
    sqlx::query("SHOW search_path")
        .fetch_one(&mut *conn)
        .await
        .expect("SHOW search_path")
        .get::<String, _>(0)
}

/// Seed one workspace schema with an endpoint, and return its name.
async fn seed_workspace(pool: &PgPool, schema: &str, endpoint: &str) {
    db::workspaces::provision_schema(pool, schema, "")
        .await
        .expect("provision schema");

    let mut conn = pool.acquire().await.expect("acquire");
    let mut ctx = DbContext::new(&mut conn, schema, "");
    db::endpoints::create(
        &mut ctx,
        endpoint,
        "HTTP",
        None,
        None,
        &serde_json::json!({ "url": "http://localhost:9999/success" }),
        None,
    )
    .await
    .expect("create endpoint");
}

#[tokio::test]
async fn workspace_queries_leave_search_path_untouched() {
    let Some(pool) = pool().await else { return };
    let schema = unique_schema("sp");
    seed_workspace(&pool, &schema, "notify").await;

    // Exercise a representative slice of the DB layer on one pooled connection.
    let mut conn = pool.acquire().await.expect("acquire");
    let observed_before = search_path_of(&mut conn).await;

    let mut ctx = DbContext::new(&mut conn, &schema, "");
    db::configs::create(&mut ctx, "cfg", &serde_json::json!({ "k": "v" }))
        .await
        .expect("create config");
    db::jobs::create_immediate(&mut ctx, "notify", "HTTP", "ikey-1", None, 1)
        .await
        .expect("create job");
    db::endpoints::list(&mut ctx, None, 10).await.expect("list endpoints");

    let observed_after = search_path_of(&mut conn).await;

    assert_eq!(
        observed_before, DEFAULT_SEARCH_PATH,
        "a freshly pooled connection must start at the server default"
    );
    assert_eq!(
        observed_after, DEFAULT_SEARCH_PATH,
        "workspace queries must not mutate session state; a `SET search_path` \
         here is what pins the connection behind RDS Proxy"
    );

    drop(conn);
    drop_schema(&pool, &schema).await;
}

/// Control for the test above: proves `search_path_of` actually detects a `SET`,
/// so a green result there means "no SET happened" rather than "the probe is blind".
#[tokio::test]
async fn search_path_probe_detects_a_real_set() {
    let Some(pool) = pool().await else { return };
    let mut conn = pool.acquire().await.expect("acquire");

    assert_eq!(search_path_of(&mut conn).await, DEFAULT_SEARCH_PATH);

    sqlx::query("SET search_path TO \"some_tenant\"")
        .execute(&mut *conn)
        .await
        .expect("SET search_path");

    // Postgres echoes the normalized value, without the redundant quotes.
    assert_eq!(
        search_path_of(&mut conn).await,
        "some_tenant",
        "the probe must observe session state, otherwise the sibling test proves nothing"
    );
}

/// sqlx issues a *named* server-side prepared statement for every parameterized
/// query; that is unavoidable short of dropping bind parameters. What is not
/// acceptable is unbounded growth, which is what disabling the statement cache
/// produces — sqlx then prepares one statement per call and never `Close`s any.
/// This pins the setting in place: repeated query shapes must be reused, and the
/// total must stay under sqlx's cache capacity rather than tracking query count.
#[tokio::test]
async fn prepared_statements_stay_bounded_and_are_reused() {
    let Some(pool) = pool().await else { return };
    let schema = unique_schema("prep");
    seed_workspace(&pool, &schema, "notify").await;

    let mut conn = pool.acquire().await.expect("acquire");
    let mut ctx = DbContext::new(&mut conn, &schema, "");

    const ITERATIONS: usize = 40;
    for i in 0..ITERATIONS {
        db::configs::create(&mut ctx, &format!("cfg-{i}"), &serde_json::json!({ "i": i }))
            .await
            .expect("create config");
        db::configs::get(&mut ctx, &format!("cfg-{i}")).await.expect("get config");
    }

    let prepared: i64 = sqlx::query("SELECT count(*) FROM pg_prepared_statements")
        .fetch_one(&mut *conn)
        .await
        .expect("count prepared statements")
        .get(0);

    // Two distinct SQL texts are executed in the loop (the insert and the
    // select); with the cache working they are prepared once each and reused.
    // A regression to `statement_cache_capacity(0)` yields ~2 * ITERATIONS.
    assert!(
        prepared < ITERATIONS as i64,
        "prepared statements grew with query count ({prepared} after {ITERATIONS} \
         iterations) — the statement cache is disabled, so sqlx is preparing one \
         named statement per call and never freeing it"
    );

    drop(conn);
    drop_schema(&pool, &schema).await;
}

#[tokio::test]
async fn reads_target_the_workspace_schema_not_public() {
    let Some(pool) = pool().await else { return };
    let schema = unique_schema("decoy");
    seed_workspace(&pool, &schema, "notify").await;

    // A decoy `public.configs` standing in for the legacy public-schema tables
    // that still exist on older deployments. An unqualified query resolving
    // through a default search_path would read this instead of the workspace.
    sqlx::query("CREATE TABLE IF NOT EXISTS public.configs (name TEXT PRIMARY KEY, values_json JSONB NOT NULL, created_at TIMESTAMPTZ NOT NULL DEFAULT now(), updated_at TIMESTAMPTZ NOT NULL DEFAULT now())")
        .execute(&pool)
        .await
        .expect("create decoy table");
    sqlx::query("INSERT INTO public.configs (name, values_json) VALUES ('decoy', '{\"leaked\":true}') ON CONFLICT DO NOTHING")
        .execute(&pool)
        .await
        .expect("seed decoy row");

    let mut conn = pool.acquire().await.expect("acquire");
    let mut ctx = DbContext::new(&mut conn, &schema, "");

    let listed = db::configs::list(&mut ctx, None, 50).await.expect("list configs");
    assert!(
        listed.is_empty(),
        "workspace was seeded with no configs, but the query returned {:?} — \
         it resolved against public instead of the workspace schema",
        listed.iter().map(|c| &c.name).collect::<Vec<_>>()
    );

    assert!(
        db::configs::get(&mut ctx, "decoy").await.expect("get decoy").is_none(),
        "the decoy row in public.configs must be invisible to a workspace-scoped read"
    );

    drop(conn);
    sqlx::query("DROP TABLE IF EXISTS public.configs")
        .execute(&pool)
        .await
        .expect("cleanup decoy");
    drop_schema(&pool, &schema).await;
}

#[tokio::test]
async fn workspaces_cannot_see_each_others_rows() {
    let Some(pool) = pool().await else { return };
    let (a, b) = (unique_schema("wsa"), unique_schema("wsb"));
    seed_workspace(&pool, &a, "notify").await;
    seed_workspace(&pool, &b, "notify").await;

    // Same connection, two tenants, interleaved — the case that silently
    // cross-reads when scope lives in connection state rather than in the SQL.
    let mut conn = pool.acquire().await.expect("acquire");

    let mut ctx_a = DbContext::new(&mut conn, &a, "");
    db::configs::create(&mut ctx_a, "only-in-a", &serde_json::json!({ "tenant": "a" }))
        .await
        .expect("create in a");

    let mut ctx_b = DbContext::new(&mut conn, &b, "");
    let leaked = db::configs::get(&mut ctx_b, "only-in-a").await.expect("get from b");
    assert!(
        leaked.is_none(),
        "workspace {b} read a row belonging to workspace {a}"
    );

    let mut ctx_a = DbContext::new(&mut conn, &a, "");
    assert!(
        db::configs::get(&mut ctx_a, "only-in-a").await.expect("get from a").is_some(),
        "workspace {a} lost its own row"
    );

    drop(conn);
    drop_schema(&pool, &a).await;
    drop_schema(&pool, &b).await;
}

#[tokio::test]
async fn provisioned_tables_land_in_the_workspace_schema() {
    let Some(pool) = pool().await else { return };
    let schema = unique_schema("ddl");
    db::workspaces::provision_schema(&pool, &schema, "")
        .await
        .expect("provision schema");

    let tables: Vec<String> = sqlx::query_scalar(
        "SELECT tablename FROM pg_tables WHERE schemaname = $1 ORDER BY tablename",
    )
    .bind(&schema)
    .fetch_all(&pool)
    .await
    .expect("list tables");

    for expected in [
        "attempts",
        "configs",
        "endpoints",
        "execution_logs",
        "executions",
        "jobs",
        "payload_specs",
        "secrets",
    ] {
        assert!(
            tables.iter().any(|t| t == expected),
            "DDL did not create {expected} inside {schema}; got {tables:?}"
        );
    }

    drop_schema(&pool, &schema).await;
}

#[tokio::test]
async fn provisioning_honours_a_table_prefix() {
    let Some(pool) = pool().await else { return };
    let schema = unique_schema("pfx");
    db::workspaces::provision_schema(&pool, &schema, "sched_")
        .await
        .expect("provision schema with prefix");

    let tables: Vec<String> = sqlx::query_scalar(
        "SELECT tablename FROM pg_tables WHERE schemaname = $1 ORDER BY tablename",
    )
    .bind(&schema)
    .fetch_all(&pool)
    .await
    .expect("list tables");

    assert!(
        tables.iter().any(|t| t == "sched_jobs"),
        "expected prefixed table sched_jobs in {schema}, got {tables:?}"
    );
    assert!(
        !tables.iter().any(|t| t == "sched__jobs"),
        "prefix was double-separated: {tables:?}"
    );

    // The read path must address the same physical tables the DDL created.
    let mut conn = pool.acquire().await.expect("acquire");
    let mut ctx = DbContext::new(&mut conn, &schema, "sched_");
    db::endpoints::create(
        &mut ctx,
        "notify",
        "HTTP",
        None,
        None,
        &serde_json::json!({ "url": "http://localhost:9999/success" }),
        None,
    )
    .await
    .expect("prefixed write must reach the provisioned table");

    drop(conn);
    drop_schema(&pool, &schema).await;
}
