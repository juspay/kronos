pub mod attempts;
pub mod configs;
pub mod endpoints;
pub mod execution_logs;
pub mod executions;
pub mod jobs;
pub mod organizations;
pub mod payload_specs;
pub mod secrets;
pub mod workspaces;

use sqlx::PgConnection;
use sqlx::PgPool;

/// Connect a `PgPool` with Kronos's logging policy: SQL statement logs are
/// demoted from sqlx's default DEBUG to TRACE so steady-state chatter (e.g.
/// the worker's poll queries) doesn't flood debug logs. Opt back in with
/// `RUST_LOG=sqlx::query=trace`.
///
/// The statement cache is deliberately left at sqlx's default rather than
/// disabled. Disabling it looks like the proxy-friendly choice — it is what the
/// equivalent diesel fix does — but sqlx behaves differently: any query carrying
/// bind parameters always goes through `get_or_prepare`, which issues a *named*
/// server-side statement regardless of the cache setting. The cache only decides
/// whether that statement is reused and later `Close`d. Turning it off therefore
/// prepares one statement per query and never frees any. Measured against
/// PostgreSQL 16 with 50 repeated queries on one connection:
///
/// | statement_cache_capacity | statements left on the session |
/// |-------------------------|--------------------------------|
/// | 0                       | 51 (unbounded — one per query) |
/// | 100 (default)           | 2 (reused), capped at 100      |
///
/// So the default is both the smaller footprint and the bounded one. Note this
/// leaves protocol-level prepared statements in play; if a proxy is observed
/// pinning on them, the lever is connection recycling (`max_lifetime`) or an
/// sqlx upgrade, not this setting.
pub async fn connect_pool(database_url: &str, max_connections: u32) -> Result<PgPool, sqlx::Error> {
    use sqlx::ConnectOptions;

    let options = database_url
        .parse::<sqlx::postgres::PgConnectOptions>()?
        .log_statements(log::LevelFilter::Trace);
    sqlx::postgres::PgPoolOptions::new()
        .max_connections(max_connections)
        .connect_with(options)
        .await
}

/// Bundles a mutable DB connection with the tenant scope every query in that
/// workspace resolves against, eliminating the need to thread three separate
/// parameters through every DB function signature.
pub struct DbContext<'a> {
    pub conn: &'a mut PgConnection,
    pub schema: &'a str,
    pub prefix: &'a str,
}

impl<'a> DbContext<'a> {
    pub fn new(conn: &'a mut PgConnection, schema: &'a str, prefix: &'a str) -> Self {
        Self {
            conn,
            schema,
            prefix,
        }
    }

    /// Schema-qualified reference to one of this workspace's tables. Preferred
    /// over the free `tbl` inside the DB layer: the schema comes from the
    /// context, so a call site cannot forget it.
    #[inline]
    pub fn tbl(&self, name: &str) -> String {
        tbl(self.schema, self.prefix, name)
    }
}

/// Build a fully schema-qualified, quoted table reference.
///
/// Tenant scope is carried in the SQL text rather than in `search_path` session
/// state. A `SET` pins the backend connection for the life of the client session
/// behind a transaction-pooling proxy (RDS Proxy, PgBouncer), which destroys
/// multiplexing — and a bare table name resolved against a stale `search_path`
/// would silently read another tenant's schema.
///
/// Both components are interpolated into SQL rather than bound, so this is an
/// injection point and validates on every call. `scoped_connection` used to hold
/// the equivalent gate before building its `SET search_path`; the check belongs
/// here now that the name reaches the query text itself. The cost is a scan of a
/// short identifier, set against a database round trip.
///
/// `tbl("ws_acme", "sched_", "jobs")` → `"ws_acme"."sched_jobs"`.
#[inline]
pub fn tbl(schema: &str, prefix: &str, name: &str) -> String {
    assert!(
        crate::tenant::validate_schema_name(schema),
        "Invalid schema name: {schema}"
    );
    assert!(
        crate::tenant::validate_table_prefix(prefix),
        "Invalid table prefix: {prefix}"
    );
    format!("\"{schema}\".\"{prefix}{name}\"")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tbl_schema_qualifies_and_quotes() {
        // Tenant scope must live in the SQL text, not in connection session state
        // (`SET search_path`), which pins connections behind RDS Proxy.
        assert_eq!(tbl("ws_acme", "", "jobs"), r#""ws_acme"."jobs""#);
    }

    #[test]
    fn tbl_applies_prefix_inside_schema() {
        assert_eq!(tbl("ws_acme", "sched_", "jobs"), r#""ws_acme"."sched_jobs""#);
    }

    #[test]
    #[should_panic(expected = "Invalid schema name")]
    fn tbl_rejects_an_unsafe_schema_name() {
        // The schema is interpolated into SQL text, so it is an injection point.
        // `scoped_connection` used to gate this on every checkout; that gate has
        // to live here now that the name reaches the query instead of a `SET`.
        tbl("ws\"; DROP TABLE jobs; --", "", "jobs");
    }

    #[test]
    #[should_panic(expected = "Invalid table prefix")]
    fn tbl_rejects_an_unsafe_table_prefix() {
        tbl("ws_acme", "bad\"prefix", "jobs");
    }

    #[test]
    fn tbl_never_emits_a_bare_table_name() {
        // A bare name would resolve through search_path — exactly the session
        // state this refactor removes. Every output must carry its schema.
        for (schema, prefix, name) in [
            ("ws_acme", "", "executions"),
            ("org_x_ws_y", "kronos_", "attempts"),
        ] {
            let out = tbl(schema, prefix, name);
            assert!(
                out.starts_with(&format!("\"{schema}\".")),
                "expected {out} to be schema-qualified"
            );
        }
    }
}
