use metrics_exporter_prometheus::PrometheusHandle;

// --- Counters ---
pub const JOBS_CREATED_TOTAL: &str = "kronos_jobs_created_total";
pub const EXECUTIONS_CLAIMED_TOTAL: &str = "kronos_executions_claimed_total";
pub const EXECUTIONS_COMPLETED_TOTAL: &str = "kronos_executions_completed_total";
pub const EXECUTIONS_PROMOTED_TOTAL: &str = "kronos_executions_promoted_total";
pub const CRON_TICKS_MATERIALIZED_TOTAL: &str = "kronos_cron_ticks_materialized_total";
pub const EXECUTIONS_RECLAIMED_TOTAL: &str = "kronos_executions_reclaimed_total";
pub const WORKER_POLL_IDLE_TOTAL: &str = "kronos_worker_poll_idle_total";

// Dispatcher-level counters
pub const DISPATCH_TOTAL: &str = "kronos_dispatch_total";
pub const KAFKA_MESSAGES_PRODUCED_TOTAL: &str = "kronos_kafka_messages_produced_total";
pub const REDIS_STREAM_MESSAGES_SENT_TOTAL: &str = "kronos_redis_stream_messages_sent_total";

// --- Histograms ---
pub const EXECUTION_DURATION_SECONDS: &str = "kronos_execution_duration_seconds";
pub const DELAYED_JOB_LAG_SECONDS: &str = "kronos_delayed_job_lag_seconds";
pub const CRON_TICK_LAG_SECONDS: &str = "kronos_cron_tick_lag_seconds";
pub const DISPATCH_DURATION_SECONDS: &str = "kronos_dispatch_duration_seconds";

// --- Gauges ---
pub const WORKER_INFLIGHT: &str = "kronos_worker_inflight_executions";

/// Install the Prometheus recorder and return a handle for rendering metrics.
/// Use this for services that already have an HTTP server (e.g. the API).
pub fn install_recorder() -> PrometheusHandle {
    metrics_exporter_prometheus::PrometheusBuilder::new()
        .install_recorder()
        .expect("failed to install Prometheus recorder")
}

/// Install the Prometheus recorder with a built-in HTTP listener.
/// Use this for services without an HTTP server (worker, scheduler).
pub fn install_recorder_with_listener(port: u16) {
    metrics_exporter_prometheus::PrometheusBuilder::new()
        .with_http_listener(([0, 0, 0, 0], port))
        .install()
        .expect("failed to install Prometheus recorder with HTTP listener");
}
