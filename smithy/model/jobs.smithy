$version: "2"

namespace com.kronos

// ─── Job structures ──────────────────────────────────────────────

structure JobResource {
    @required
    job_id: String

    @required
    endpoint: String

    @required
    endpoint_type: EndpointTypeEnum

    @required
    trigger_type: TriggerTypeEnum

    @required
    status: JobStatusEnum

    @required
    version: Integer

    previous_version_id: String

    replaced_by_id: String

    idempotency_key: String

    input: JsonObject

    run_at: DateTime

    cron_expression: String

    cron_timezone: String

    cron_starts_at: DateTime

    cron_ends_at: DateTime

    cron_next_run_at: DateTime

    cron_last_tick_at: DateTime

    @required
    created_at: DateTime

    retired_at: DateTime
}

structure JobSummary {
    @required
    job_id: String

    @required
    endpoint: String

    @required
    trigger_type: TriggerTypeEnum

    @required
    status: JobStatusEnum

    @required
    version: Integer

    @required
    created_at: DateTime
}

list JobSummaryList {
    member: JobSummary
}

list JobVersionList {
    member: JobResource
}

// ─── Job Status ──────────────────────────────────────────────────

structure JobStatusResponse {
    @required
    job_id: String

    @required
    job_status: JobStatusEnum

    latest_execution: ExecutionSummary
}

structure ExecutionSummary {
    @required
    execution_id: String

    @required
    status: ExecutionStatusEnum

    @required
    attempt_count: Integer

    @required
    max_attempts: Integer

    started_at: DateTime

    completed_at: DateTime
}

// ─── Create ──────────────────────────────────────────────────────

@input
structure CreateJobInput with [WorkspaceHeaders] {
    @required
    endpoint: String

    @required
    trigger: TriggerTypeEnum

    idempotency_key: String

    input: JsonObject

    run_at: DateTime

    cron: String

    timezone: String

    starts_at: DateTime

    ends_at: DateTime
}

structure CreateJobOutput {
    @required
    data: JobResource
}

@http(method: "POST", uri: "/v1/jobs", code: 201)
operation CreateJob {
    input: CreateJobInput
    output: CreateJobOutput
    errors: [InvalidRequestError, ConflictError, NotFoundError, UnprocessableEntityError]
}

// ─── List ────────────────────────────────────────────────────────

@input
structure ListJobsInput with [WorkspaceHeaders, PaginationQuery] {
    @httpQuery("endpoint")
    endpoint: String

    @httpQuery("trigger_type")
    trigger_type: TriggerTypeEnum

    @httpQuery("status")
    status: JobStatusEnum
}

structure ListJobsOutput {
    @required
    data: JobSummaryList

    cursor: String
}

@http(method: "GET", uri: "/v1/jobs", code: 200)
@readonly
operation ListJobs {
    input: ListJobsInput
    output: ListJobsOutput
}

// ─── Get ─────────────────────────────────────────────────────────

@input
structure GetJobInput with [WorkspaceHeaders] {
    @required
    @httpLabel
    job_id: String
}

structure GetJobOutput {
    @required
    data: JobResource
}

@http(method: "GET", uri: "/v1/jobs/{job_id}", code: 200)
@readonly
operation GetJob {
    input: GetJobInput
    output: GetJobOutput
    errors: [NotFoundError]
}

// ─── Update ──────────────────────────────────────────────────────

@input
structure UpdateJobInput with [WorkspaceHeaders] {
    @required
    @httpLabel
    job_id: String

    cron: String

    timezone: String

    input: JsonObject

    starts_at: DateTime

    ends_at: DateTime
}

structure UpdateJobOutput {
    @required
    data: JobResource
}

@http(method: "PUT", uri: "/v1/jobs/{job_id}", code: 200)
@idempotent
operation UpdateJob {
    input: UpdateJobInput
    output: UpdateJobOutput
    errors: [NotFoundError, InvalidRequestError, UnprocessableEntityError]
}

// ─── Cancel ──────────────────────────────────────────────────────

@input
structure CancelJobInput with [WorkspaceHeaders] {
    @required
    @httpLabel
    job_id: String
}

structure CancelJobOutput {
    @required
    data: JobResource
}

@http(method: "POST", uri: "/v1/jobs/{job_id}/cancel", code: 200)
operation CancelJob {
    input: CancelJobInput
    output: CancelJobOutput
    errors: [NotFoundError, ConflictError]
}

// ─── Get Status ──────────────────────────────────────────────────

@input
structure GetJobStatusInput with [WorkspaceHeaders] {
    @required
    @httpLabel
    job_id: String
}

structure GetJobStatusOutput {
    @required
    data: JobStatusResponse
}

@http(method: "GET", uri: "/v1/jobs/{job_id}/status", code: 200)
@readonly
operation GetJobStatus {
    input: GetJobStatusInput
    output: GetJobStatusOutput
    errors: [NotFoundError]
}

// ─── Get Versions ────────────────────────────────────────────────

@input
structure GetJobVersionsInput with [WorkspaceHeaders] {
    @required
    @httpLabel
    job_id: String
}

structure GetJobVersionsOutput {
    @required
    data: JobVersionList
}

@http(method: "GET", uri: "/v1/jobs/{job_id}/versions", code: 200)
@readonly
operation GetJobVersions {
    input: GetJobVersionsInput
    output: GetJobVersionsOutput
    errors: [NotFoundError]
}

// ─── List Job Executions ─────────────────────────────────────────

@input
structure ListJobExecutionsInput with [WorkspaceHeaders, PaginationQuery] {
    @required
    @httpLabel
    job_id: String

    @httpQuery("status")
    status: ExecutionStatusEnum
}

structure ListJobExecutionsOutput {
    @required
    data: ExecutionResourceList

    cursor: String
}

@http(method: "GET", uri: "/v1/jobs/{job_id}/executions", code: 200)
@readonly
operation ListJobExecutions {
    input: ListJobExecutionsInput
    output: ListJobExecutionsOutput
    errors: [NotFoundError]
}
