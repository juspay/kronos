use metrics_exporter_prometheus::PrometheusHandle;

// --- Counters ---
pub const JOBS_CREATED_TOTAL: &str = "kronos_jobs_created_total";
pub const EXECUTIONS_CLAIMED_TOTAL: &str = "kronos_executions_claimed_total";
pub const EXECUTIONS_COMPLETED_TOTAL: &str = "kronos_executions_completed_total";
pub const WORKER_POLL_IDLE_TOTAL: &str = "kronos_worker_poll_idle_total";
pub const CRON_JOBS_REAPED_TOTAL: &str = "kronos_cron_jobs_reaped_total";

// Dispatcher-level counters
pub const DISPATCH_TOTAL: &str = "kronos_dispatch_total";
pub const KAFKA_MESSAGES_PRODUCED_TOTAL: &str = "kronos_kafka_messages_produced_total";
pub const REDIS_STREAM_MESSAGES_SENT_TOTAL: &str = "kronos_redis_stream_messages_sent_total";

// --- Histograms ---
pub const EXECUTION_DURATION_SECONDS: &str = "kronos_execution_duration_seconds";
pub const DISPATCH_DURATION_SECONDS: &str = "kronos_dispatch_duration_seconds";

// --- Gauges ---
pub const WORKER_INFLIGHT: &str = "kronos_worker_inflight_executions";
pub const EXECUTIONS_WAITING: &str = "kronos_executions_waiting";

// --- Long-running counters / histograms ---
pub const POLLS_TOTAL: &str = "kronos_polls_total";
pub const POLL_DURATION_SECONDS: &str = "kronos_poll_duration_seconds";
pub const CALLBACKS_RECEIVED_TOTAL: &str = "kronos_callbacks_received_total";
pub const LONG_RUNNING_COMPLETED_TOTAL: &str = "kronos_long_running_completed_total";

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
