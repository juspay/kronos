pub mod attempts;
pub mod configs;
pub mod endpoints;
pub mod execution_logs;
pub mod executions;
pub mod jobs;
pub mod organizations;
pub mod payload_specs;
pub mod scoped;
pub mod secrets;
pub mod workspaces;

use sqlx::PgConnection;

/// Bundles a mutable DB connection and its table prefix, eliminating the need
/// to thread two separate parameters through every DB function signature.
pub struct DbContext<'a> {
    pub conn: &'a mut PgConnection,
    pub prefix: &'a str,
}

impl<'a> DbContext<'a> {
    pub fn new(conn: &'a mut PgConnection, prefix: &'a str) -> Self {
        Self { conn, prefix }
    }
}

/// Build a (potentially prefixed) table name.
/// `tbl("sched", "jobs")` → `"sched_jobs"`, `tbl("", "jobs")` → `"jobs"`.
#[inline]
pub fn tbl(prefix: &str, name: &str) -> String {
    if prefix.is_empty() {
        name.to_string()
    } else {
        format!("{prefix}_{name}")
    }
}
