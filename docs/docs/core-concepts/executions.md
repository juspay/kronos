---
id: executions
title: Executions
---

# Executions

An execution is a materialized instance of a job. When a job fires, Kronos creates an execution row in the database representing a single delivery attempt to the endpoint. Each execution has a lifecycle, can be retried, and produces one or more attempt records.

---

## What is an execution?

Every time a job fires — whether immediately, on a delay, or on a CRON tick — Kronos creates an **execution**. The execution represents the full lifecycle of delivering the job to its endpoint, from initial creation through to final success or failure.

- A one-shot job (`IMMEDIATE` or `DELAYED`) produces exactly one execution.
- A `CRON` job produces a new execution on each tick of its schedule.
- Each execution can have multiple **attempts** if the endpoint's retry policy allows retries.

---

## Execution lifecycle

Executions move through a state machine:

```
PENDING ──→ QUEUED ──→ RUNNING ──→ SUCCESS
               │          │
               │          ├──→ RETRYING ──→ RUNNING (next attempt)
               │          │
               │          └──→ FAILED (retries exhausted)
               │
               └──→ CANCELLED
```

| State | Description |
|-------|-------------|
| `PENDING` | Created for `DELAYED` jobs. Waiting for `run_at` to arrive. Workers pick it up once `run_at <= now()`. |
| `QUEUED` | Ready for execution. Created for `IMMEDIATE` jobs (same transaction as the job) and `CRON` ticks (inserted by pg_cron). Workers claim it via `SELECT FOR UPDATE SKIP LOCKED`. |
| `RUNNING` | A worker has claimed the execution and is actively dispatching it to the endpoint. |
| `RETRYING` | The attempt failed and retries remain. The execution is scheduled to run again after the computed backoff delay. |
| `SUCCESS` | The dispatch succeeded (e.g. HTTP response status in `expected_status_codes`). Terminal state. |
| `FAILED` | All retry attempts have been exhausted without success. Terminal state. |
| `CANCELLED` | The execution was cancelled while `PENDING` or `QUEUED`. Terminal state. |

:::note
`RUNNING` executions that exceed the stuck execution timeout (default: 300 seconds) are automatically reclaimed by the stuck execution reclaimer. They are set to `RETRYING` (if retries remain) or `FAILED` (if retries are exhausted).
:::

---

## How trigger types create executions

The trigger type of a job determines the initial state of its execution:

| Trigger | Initial execution state | Created by | Picked up when |
|---------|------------------------|------------|----------------|
| `IMMEDIATE` | `QUEUED` | API server (same transaction as job creation) | Immediately by workers |
| `DELAYED` | `PENDING` | API server (same transaction as job creation) | When `run_at <= now()` — no separate promoter needed |
| `CRON` | `QUEUED` | pg_cron (on each tick) | Immediately by workers |

For `DELAYED` jobs, the worker's claim query includes `PENDING` status with `run_at <= now()`, so delayed jobs are picked up directly when their time arrives. The pickup index covers all three actionable statuses: `WHERE status IN ('QUEUED', 'RETRYING', 'PENDING')`.

---

## Execution fields

| Field | Type | Description |
|-------|------|-------------|
| `execution_id` | string | Unique identifier (UUID). |
| `job_id` | string | The job that produced this execution. |
| `endpoint` | string | The endpoint name to deliver to. |
| `endpoint_type` | string | `HTTP`, `KAFKA`, `REDIS_STREAM`. |
| `idempotency_key` | string | Deduplication key. For CRON: `cron_{job_id}_{epoch_ms}`. |
| `status` | string | Current lifecycle state (see above). |
| `input` | object | The job's input payload (copied at execution creation time). |
| `output` | object | The dispatch result (e.g. `{"status_code": 200, "body": "OK"}` for HTTP). |
| `attempt_count` | integer | Number of attempts made so far. |
| `max_attempts` | integer | Maximum attempts from the endpoint's retry policy. |
| `worker_id` | string | ID of the worker that claimed this execution (null when not running). |
| `run_at` | ISO 8601 | When the execution should be picked up. For `RETRYING`, this is `now() + backoff`. |
| `started_at` | ISO 8601 | When the worker started processing. |
| `completed_at` | ISO 8601 | When the execution reached a terminal state. |
| `duration_ms` | integer | End-to-end execution duration in milliseconds. |
| `created_at` | ISO 8601 | Execution creation timestamp. |

---

## Attempts

Each retry within an execution creates an **attempt** record. An attempt captures the outcome of a single dispatch try:

| Field | Type | Description |
|-------|------|-------------|
| `attempt_id` | string | Unique identifier (UUID). |
| `execution_id` | string | The parent execution. |
| `attempt_number` | integer | Sequential attempt number (1, 2, 3, ...). |
| `status` | string | `SUCCESS` or `FAILED`. |
| `started_at` | ISO 8601 | When the attempt started. |
| `completed_at` | ISO 8601 | When the attempt completed. |
| `duration_ms` | integer | Attempt duration in milliseconds. |
| `output` | object | Dispatch output on success (transport-specific). |
| `error` | object | Error details on failure (`{ "type": "...", "message": "..." }`). |
| `created_at` | ISO 8601 | Attempt record creation timestamp. |

### Output shapes by transport

| Transport | Output shape |
|-----------|-------------|
| HTTP | `{ "status_code": 200, "body": "OK" }` |
| Kafka | `{ "partition": 3, "offset": 12847 }` |
| Redis | `{ "message_id": "1710499801234-0", "stream": "notifications:outbound" }` |

### Error shapes by transport

| Transport | Error types |
|-----------|-------------|
| HTTP | `TIMEOUT`, `HTTP_ERROR` (with `status_code`), `CONNECTION_ERROR` |
| Kafka | `BROKER_ERROR`, `TIMEOUT` |
| Redis | `CONNECTION_ERROR`, `TIMEOUT`, `STREAM_ERROR` |

Example HTTP error:

```json
{
  "type": "HTTP_ERROR",
  "status_code": 503,
  "message": "Unexpected status code: 503"
}
```

---

## Viewing executions and attempts

### Get execution details

```bash
curl http://localhost:8080/v1/executions/{execution_id} \
  -H "Authorization: Bearer dev-api-key" \
  -H "X-Org-Id: <org_id>" \
  -H "X-Workspace-Id: <workspace_id>"
```

### List executions for a job

```bash
curl "http://localhost:8080/v1/jobs/{job_id}/executions?limit=50" \
  -H "Authorization: Bearer dev-api-key" \
  -H "X-Org-Id: <org_id>" \
  -H "X-Workspace-Id: <workspace_id>"
```

### List attempt history

```bash
curl http://localhost:8080/v1/executions/{execution_id}/attempts \
  -H "Authorization: Bearer dev-api-key" \
  -H "X-Org-Id: <org_id>" \
  -H "X-Workspace-Id: <workspace_id>"
```

### List execution logs

```bash
curl http://localhost:8080/v1/executions/{execution_id}/logs \
  -H "Authorization: Bearer dev-api-key" \
  -H "X-Org-Id: <org_id>" \
  -H "X-Workspace-Id: <workspace_id>"
```

---

## Cancelling an execution

```bash
curl -X POST http://localhost:8080/v1/executions/{execution_id}/cancel \
  -H "Authorization: Bearer dev-api-key" \
  -H "X-Org-Id: <org_id>" \
  -H "X-Workspace-Id: <workspace_id>"
```

An execution can only be cancelled if it is in `PENDING` or `QUEUED` status. If already `RUNNING`, the API returns `409 EXECUTION_NOT_CANCELLABLE`.

---

## See also

- [Jobs](./jobs) — trigger types and job lifecycle
- [Retry Policy](./retry-policy) — backoff strategies and jitter
- [Idempotency](./idempotency) — CRON tick deduplication
- [Templates](./templates) — how execution input is resolved
