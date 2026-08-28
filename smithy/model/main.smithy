$version: "2"

namespace com.invokr

use aws.protocols#restJson1

/// Invokr — Distributed Job Scheduling and Execution Engine.
/// Provides durable, retriable delivery of messages to HTTP endpoints,
/// Kafka topics, and Redis Streams.
@restJson1
@title("Invokr API")
@httpBearerAuth
service InvokrService {
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
