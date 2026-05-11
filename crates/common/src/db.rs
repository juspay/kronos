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
