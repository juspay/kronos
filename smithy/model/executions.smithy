$version: "2"

namespace com.kronos

// ─── Execution structures ────────────────────────────────────────

structure ExecutionResource {
    @required
    execution_id: String

    @required
    job_id: String

    @required
    endpoint: String

    @required
    endpoint_type: EndpointTypeEnum

    idempotency_key: String

    @required
    status: ExecutionStatusEnum

    input: JsonObject

    output: JsonObject

    @required
    attempt_count: Integer

    @required
    max_attempts: Integer

    worker_id: String

    @required
    run_at: DateTime

    started_at: DateTime

    completed_at: DateTime

    duration_ms: Integer

    @required
    created_at: DateTime
}

list ExecutionResourceList {
    member: ExecutionResource
}

// ─── Attempt structures ──────────────────────────────────────────

structure AttemptResource {
    @required
    attempt_id: String

    @required
    execution_id: String

    @required
    attempt_number: Integer

    @required
    status: AttemptStatusEnum

    @required
    started_at: DateTime

    completed_at: DateTime

    duration_ms: Integer

    output: JsonObject

    error: JsonObject

    @required
    created_at: DateTime
}

list AttemptResourceList {
    member: AttemptResource
}

// ─── Execution Log structures ────────────────────────────────────

structure ExecutionLogResource {
    @required
    log_id: String

    @required
    execution_id: String

    @required
    attempt_number: Integer

    @required
    level: LogLevelEnum

    @required
    message: String

    @required
    logged_at: DateTime
}

list ExecutionLogList {
    member: ExecutionLogResource
}

// ─── Get Execution ───────────────────────────────────────────────

structure GetExecutionInput {
    @required
    @httpLabel
    execution_id: String
}

structure GetExecutionOutput {
    @required
    data: ExecutionResource
}

@http(method: "GET", uri: "/v1/executions/{execution_id}", code: 200)
@readonly
operation GetExecution {
    input: GetExecutionInput
    output: GetExecutionOutput
    errors: [NotFoundError]
}

// ─── Cancel Execution ────────────────────────────────────────────

structure CancelExecutionInput {
    @required
    @httpLabel
    execution_id: String
}

structure CancelExecutionOutput {
    @required
    data: ExecutionResource
}

@http(method: "POST", uri: "/v1/executions/{execution_id}/cancel", code: 200)
operation CancelExecution {
    input: CancelExecutionInput
    output: CancelExecutionOutput
    errors: [NotFoundError, ConflictError]
}

// ─── List Attempts ───────────────────────────────────────────────

@input
structure ListExecutionAttemptsInput with [PaginationQuery] {
    @required
    @httpLabel
    execution_id: String
}

structure ListExecutionAttemptsOutput {
    @required
    data: AttemptResourceList

    cursor: String
}

@http(method: "GET", uri: "/v1/executions/{execution_id}/attempts", code: 200)
@readonly
operation ListExecutionAttempts {
    input: ListExecutionAttemptsInput
    output: ListExecutionAttemptsOutput
    errors: [NotFoundError]
}

// ─── List Execution Logs ─────────────────────────────────────────

@input
structure ListExecutionLogsInput with [PaginationQuery] {
    @required
    @httpLabel
    execution_id: String
}

structure ListExecutionLogsOutput {
    @required
    data: ExecutionLogList

    cursor: String
}

@http(method: "GET", uri: "/v1/executions/{execution_id}/logs", code: 200)
@readonly
operation ListExecutionLogs {
    input: ListExecutionLogsInput
    output: ListExecutionLogsOutput
    errors: [NotFoundError]
}
