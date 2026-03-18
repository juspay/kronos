$version: "2"

namespace com.kronos

use aws.protocols#restJson1

/// Kronos — Distributed Job Scheduling and Execution Engine.
/// Provides durable, retriable delivery of messages to HTTP endpoints,
/// Kafka topics, and Redis Streams.
@restJson1
@title("Kronos Task Executor API")
@httpBearerAuth
service KronosService {
    version: "2026-03-17"
    operations: [
        // Payload Specs
        CreatePayloadSpec
        ListPayloadSpecs
        GetPayloadSpec
        UpdatePayloadSpec
        DeletePayloadSpec

        // Endpoints
        CreateEndpoint
        ListEndpoints
        GetEndpoint
        UpdateEndpoint
        DeleteEndpoint

        // Jobs
        CreateJob
        ListJobs
        GetJob
        UpdateJob
        CancelJob
        GetJobStatus
        GetJobVersions
        ListJobExecutions

        // Executions
        GetExecution
        CancelExecution
        ListExecutionAttempts
        ListExecutionLogs
    ]
    errors: [
        UnauthorizedError
        InternalError
        RateLimitedError
    ]
}
