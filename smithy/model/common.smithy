$version: "2"

namespace com.kronos

// ─── Pagination ────────────────────────────────────────────

@mixin
structure PaginationQuery {
    @httpQuery("cursor")
    cursor: String

    @httpQuery("limit")
    @range(min: 1, max: 200)
    limit: Integer
}

// ─── Timestamps ────────────────────────────────────────────

@timestampFormat("date-time")
timestamp DateTime

// ─── Shared enums ──────────────────────────────────────────

enum EndpointTypeEnum {
    HTTP
    KAFKA
    REDIS_STREAM
}

enum TriggerTypeEnum {
    IMMEDIATE
    DELAYED
    CRON
}

enum JobStatusEnum {
    ACTIVE
    RETIRED
}

enum ExecutionStatusEnum {
    PENDING
    QUEUED
    RUNNING
    RETRYING
    SUCCESS
    FAILED
    CANCELLED
}

enum AttemptStatusEnum {
    SUCCESS
    FAILED
}

enum HealthEnum {
    HEALTHY
    DEGRADED
    FAILING
    IDLE
}

enum BackoffTypeEnum {
    FIXED = "fixed"
    LINEAR = "linear"
    EXPONENTIAL = "exponential"
}

enum LogLevelEnum {
    DEBUG
    INFO
    WARN
    ERROR
}

// ─── Retry Policy (shared) ─────────────────────────────────

structure RetryPolicy {
    @documentation("Total attempts including first. Default: 1")
    max_attempts: Integer

    @documentation("fixed | linear | exponential. Default: exponential")
    backoff: BackoffTypeEnum

    @documentation("Delay before first retry in ms. Default: 1000")
    initial_delay_ms: Long

    @documentation("Upper bound on backoff in ms. Default: 60000")
    max_delay_ms: Long
}

// ─── Error structures ──────────────────────────────────────

structure ErrorBody {
    @required
    error: ErrorDetail
}

structure ErrorDetail {
    @required
    code: String

    @required
    message: String

    request_id: String
}

@error("client")
@httpError(400)
structure InvalidRequestError {
    @required
    error: ErrorDetail
}

@error("client")
@httpError(401)
structure UnauthorizedError {
    @required
    error: ErrorDetail
}

@error("client")
@httpError(404)
structure NotFoundError {
    @required
    error: ErrorDetail
}

@error("client")
@httpError(409)
structure ConflictError {
    @required
    error: ErrorDetail
}

@error("client")
@httpError(422)
structure UnprocessableEntityError {
    @required
    error: ErrorDetail
}

@error("client")
@httpError(429)
structure RateLimitedError {
    @required
    error: ErrorDetail
}

@error("server")
@httpError(500)
structure InternalError {
    @required
    error: ErrorDetail
}

// ─── JSON document type ────────────────────────────────────

document JsonObject
