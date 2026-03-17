# Task Executor — Implementation Brief for Rust + CockroachDB

## What to Build

Build a **Task Executor** service in Rust — a distributed job scheduling and execution engine that provides durable, exactly-once, retriable delivery of messages to HTTP endpoints, Kafka topics, and Redis Streams. Think of it as `setTimeout` and `setInterval` as a service.

The service runs on **tokio** async runtime, uses **CockroachDB** (PostgreSQL-compatible) as the primary datastore, and exposes a REST API.

**For now, deploy in a single region.** The schema and code should be multi-region-aware (using `crdb_region` columns, region-aware queries), but the health checker and failover logic should be stubbed/deferred — not deleted, just inactive. When we add a second region later, we activate it.

Use cargo-workspaces to organize the codebase. Add a nix flake for the project, and a docker-compose for local development.

---

## Conceptual Model

| JS Primitive | Task Executor | Trigger |
|---|---|---|
| `setTimeout(fn, 0)` | `POST /jobs { trigger: IMMEDIATE }` | Fire once, now |
| `setTimeout(fn, delay)` | `POST /jobs { trigger: DELAYED }` | Fire once, later |
| `setInterval(fn, interval)` | `POST /jobs { trigger: CRON }` | Fire repeatedly |
| `clearTimeout` / `clearInterval` | `POST /jobs/{id}/cancel` | Stop |

The platform adds: durability, distributed execution, retry with backoff, exactly-once guarantees, and full observability.

---

## Three-Step Model

| Step | What | Endpoint |
|------|------|----------|
| **1. Setup** | Create configs, secrets, and payload specs | `/configs`, `/secrets`, `/payload-specs` |
| **2. Register** | Register an endpoint (HTTP, Kafka, or Redis Stream) | `POST /endpoints` |
| **3. Invoke** | Fire the endpoint — now, later, or on a schedule | `POST /jobs` |

---

## Domain Model

| Entity | Description |
|--------|-------------|
| **PayloadSpec** | A JSON Schema defining the input contract for an endpoint. Validated at job creation time. |
| **Config** | A key-value object holding static variables (base URLs, topic names). Referenced by endpoints, resolved at execution runtime. |
| **Secret** | A sensitive value (API key, credential). Referenced via `{{secret.*}}`. Resolved at runtime, never exposed in API responses. |
| **Endpoint** | A registered delivery target — HTTP URL, Kafka topic, or Redis Stream. Defines where to send, the message shape, and retry policy. Types: `HTTP`, `KAFKA`, `REDIS_STREAM`. |
| **Job** | An invocation of an endpoint. Creating a job triggers execution. One-shot jobs (`IMMEDIATE`, `DELAYED`) fire once. `CRON` jobs generate executions on a schedule until cancelled. Jobs are **immutable**. CRON jobs are "updated" by creating a new version and retiring the old one. |
| **Execution** | A single delivery attempt to the endpoint. Each job fire produces one execution with lifecycle: `PENDING → QUEUED → RUNNING → SUCCESS / FAILED`. |
| **Attempt** | A single try within an execution. Failed attempts retry per the endpoint's retry policy. |

---

## Template Resolution

Endpoint specs use template variables resolved from three namespaces:

| Namespace | Source | Resolved when | Per-execution? |
|---|---|---|---|
| `{{input.*}}` | Job input | Execution runtime | Yes |
| `{{config.*}}` | Endpoint's referenced config | Execution runtime | No |
| `{{secret.*}}` | Secret store | Execution runtime | No |

At execution runtime, the worker resolves `{{config.*}}` and `{{secret.*}}` first, then `{{input.*}}`. If any variable is unresolvable, the execution fails immediately.

---

## Deduplication

Single mechanism: **unique constraint on `(endpoint, idempotency_key)`**.

| Trigger | Key provided by | Example |
|---------|----------------|---------|
| `IMMEDIATE` / `DELAYED` | Client | `order-1234-welcome-email` |
| `CRON` | System | `job_{id}_{epoch_ms}` |

Duplicate requests return the existing entity with `200 OK` instead of `201 Created`.

---

## API Specification

### Authentication

All requests require `Authorization: Bearer <api_key>`.

### Step 1 — Setup

#### Payload Specs

```
POST   /payload-specs           Create a payload spec
GET    /payload-specs           List all payload specs
GET    /payload-specs/{name}    Get a payload spec
PUT    /payload-specs/{name}    Update a payload spec
DELETE /payload-specs/{name}    Delete (fails if endpoints reference it)
```

**Create request:**
```json
{
  "name": "send-welcome-email-input",
  "schema": {
    "type": "object",
    "properties": {
      "user_id": { "type": "string", "description": "Target user ID" },
      "order_id": { "type": "string", "description": "Associated order ID" }
    },
    "required": ["user_id"]
  }
}
```

**Response (201):**
```json
{
  "name": "send-welcome-email-input",
  "schema": { ... },
  "created_at": "2026-03-15T10:00:00Z",
  "updated_at": "2026-03-15T10:00:00Z"
}
```

#### Configs

```
POST   /configs           Create a config
GET    /configs           List all configs
GET    /configs/{name}    Get a config
PUT    /configs/{name}    Update a config
DELETE /configs/{name}    Delete (fails if endpoints reference it)
```

**Create request:**
```json
{
  "name": "email-service",
  "values": {
    "api_base_url": "https://api.myapp.com",
    "sender": "noreply@myapp.com"
  }
}
```

Kafka config example:
```json
{
  "name": "order-events",
  "values": {
    "bootstrap_servers": "kafka-1:9092,kafka-2:9092",
    "topic": "order.events.v1"
  }
}
```

Redis Stream config example:
```json
{
  "name": "notification-stream",
  "values": {
    "redis_url": "redis://redis-cluster:6379",
    "stream_name": "notifications:outbound",
    "max_stream_length": 100000
  }
}
```

Config updates take effect for future executions. In-flight executions use the config snapshot from when they started.

#### Secrets

```
POST   /secrets           Create a secret
GET    /secrets           List all secrets (names only, no values)
GET    /secrets/{name}    Get secret metadata (no value)
PUT    /secrets/{name}    Rotate / update a secret value
DELETE /secrets/{name}    Delete (fails if endpoints reference it)
```

**Create request:**
```json
{
  "name": "email_api_key",
  "value": "sk-live-abc123..."
}
```

**Response (201) — value is NEVER returned:**
```json
{
  "name": "email_api_key",
  "created_at": "2026-03-15T10:00:00Z",
  "updated_at": "2026-03-15T10:00:00Z"
}
```

### Step 2 — Register

```
POST   /endpoints           Register an endpoint
GET    /endpoints           List all endpoints
GET    /endpoints/{name}    Get an endpoint
PUT    /endpoints/{name}    Update (applies to future jobs only)
DELETE /endpoints/{name}    Delete (fails if active jobs reference it)
```

**HTTP endpoint:**
```json
{
  "name": "send-welcome-email",
  "type": "HTTP",
  "payload_spec": "send-welcome-email-input",
  "config": "email-service",
  "spec": {
    "url": "{{config.api_base_url}}/emails/welcome",
    "method": "POST",
    "headers": {
      "Authorization": "Bearer {{secret.email_api_key}}",
      "Content-Type": "application/json"
    },
    "body_template": {
      "user_id": "{{input.user_id}}",
      "sender": "{{config.sender}}"
    },
    "timeout_ms": 5000,
    "expected_status_codes": [200, 201, 202, 204]
  },
  "retry_policy": {
    "max_attempts": 3,
    "backoff": "exponential",
    "initial_delay_ms": 1000,
    "max_delay_ms": 30000
  }
}
```

**Kafka endpoint:**
```json
{
  "name": "publish-order-event",
  "type": "KAFKA",
  "payload_spec": "order-event-input",
  "config": "order-events",
  "spec": {
    "bootstrap_servers": "{{config.bootstrap_servers}}",
    "topic": "{{config.topic}}",
    "key_template": "{{input.order_id}}",
    "value_template": {
      "event_type": "{{input.event_type}}",
      "order_id": "{{input.order_id}}",
      "amount": "{{input.amount}}"
    },
    "headers": {
      "ce-type": "order.{{input.event_type}}",
      "ce-source": "task-executor"
    },
    "acks": "all",
    "timeout_ms": 10000
  },
  "retry_policy": {
    "max_attempts": 5,
    "backoff": "exponential",
    "initial_delay_ms": 500,
    "max_delay_ms": 15000
  }
}
```

**Redis Stream endpoint:**
```json
{
  "name": "push-notification",
  "type": "REDIS_STREAM",
  "payload_spec": "notification-input",
  "config": "notification-stream",
  "spec": {
    "redis_url": "{{config.redis_url}}",
    "stream": "{{config.stream_name}}",
    "fields_template": {
      "user_id": "{{input.user_id}}",
      "title": "{{input.title}}",
      "body": "{{input.body}}"
    },
    "max_len": "{{config.max_stream_length}}",
    "approximate_trimming": true,
    "timeout_ms": 3000
  },
  "retry_policy": {
    "max_attempts": 3,
    "backoff": "exponential",
    "initial_delay_ms": 500,
    "max_delay_ms": 10000
  }
}
```

**Endpoint field reference:**

| Field | Type | Required | Description |
|-------|------|:--------:|-------------|
| `name` | string | ✓ | Unique, URL-safe (lowercase alphanumeric, hyphens). |
| `type` | string | ✓ | `HTTP`, `KAFKA`, `REDIS_STREAM`. |
| `payload_spec` | string | | Name of a registered payload spec. Enables input validation. |
| `config` | string | | Name of a registered config. Values available as `{{config.*}}`. |
| `spec` | object | ✓ | Transport-specific. See examples above. |
| `retry_policy` | object | | Retry behavior. |

**`spec` for HTTP:**

| Field | Type | Required | Description |
|-------|------|:--------:|-------------|
| `url` | string | ✓ | Supports `{{config.*}}`, `{{secret.*}}`. |
| `method` | string | ✓ | `GET`, `POST`, `PUT`, `PATCH`, `DELETE`. |
| `headers` | map | | Supports `{{config.*}}`, `{{secret.*}}`. |
| `body_template` | object | | Supports `{{input.*}}`, `{{config.*}}`, `{{secret.*}}`. |
| `timeout_ms` | integer | ✓ | Request timeout. |
| `expected_status_codes` | int[] | | Default: `[200, 201, 202, 204]`. |

**`spec` for KAFKA:**

| Field | Type | Required | Description |
|-------|------|:--------:|-------------|
| `bootstrap_servers` | string | ✓ | Supports `{{config.*}}`, `{{secret.*}}`. |
| `topic` | string | ✓ | Supports `{{config.*}}`. |
| `key_template` | string | | Supports `{{input.*}}`, `{{config.*}}`. |
| `value_template` | object | ✓ | Supports `{{input.*}}`, `{{config.*}}`, `{{secret.*}}`. |
| `headers` | map | | Supports `{{config.*}}`, `{{secret.*}}`. |
| `acks` | string | | `"0"`, `"1"`, `"all"`. Default: `"all"`. |
| `timeout_ms` | integer | | Default: `10000`. |

**`spec` for REDIS_STREAM:**

| Field | Type | Required | Description |
|-------|------|:--------:|-------------|
| `redis_url` | string | ✓ | Supports `{{config.*}}`, `{{secret.*}}`. |
| `stream` | string | ✓ | Supports `{{config.*}}`. |
| `fields_template` | object | ✓ | Supports `{{input.*}}`, `{{config.*}}`, `{{secret.*}}`. |
| `max_len` | integer | | MAXLEN for trimming. |
| `approximate_trimming` | boolean | | Default: `true`. |
| `timeout_ms` | integer | | Default: `3000`. |

**`retry_policy` (shared):**

| Field | Type | Default | Description |
|-------|------|:-------:|-------------|
| `max_attempts` | integer | `1` | Total attempts including first. |
| `backoff` | string | `exponential` | `fixed`, `linear`, `exponential`. |
| `initial_delay_ms` | integer | `1000` | Delay before first retry. |
| `max_delay_ms` | integer | `60000` | Upper bound on backoff. |

### Step 3 — Invoke

```
POST   /jobs                    Create a job (triggers execution)
GET    /jobs                    List jobs
GET    /jobs/{job_id}           Get a job
PUT    /jobs/{job_id}           Update CRON job (creates new version)
POST   /jobs/{job_id}/cancel    Cancel / retire a job
GET    /jobs/{job_id}/status    Job health and stats (CRON jobs)
GET    /jobs/{job_id}/versions  Version history (CRON jobs)
```

**Immediate — `setTimeout(fn, 0)`:**
```json
{
  "endpoint": "send-welcome-email",
  "trigger": "IMMEDIATE",
  "idempotency_key": "order-1234-welcome-email",
  "input": {
    "user_id": "u_abc",
    "order_id": "order-1234"
  }
}
```

**Delayed — `setTimeout(fn, delay)`:**
```json
{
  "endpoint": "send-welcome-email",
  "trigger": "DELAYED",
  "idempotency_key": "order-1234-reminder",
  "run_at": "2026-03-15T18:00:00Z",
  "input": {
    "user_id": "u_abc",
    "order_id": "order-1234"
  }
}
```

**Cron — `setInterval(fn, interval)`:**
```json
{
  "endpoint": "push-notification",
  "trigger": "CRON",
  "cron": "0 9 * * MON",
  "timezone": "Asia/Kolkata",
  "starts_at": "2026-03-16T00:00:00Z",
  "ends_at": null,
  "input": {
    "user_id": "u_abc",
    "title": "Weekly Summary",
    "body": "Here's your weekly report"
  }
}
```

**Response — Immediate/Delayed (201):**
```json
{
  "job_id": "job_8f3a...",
  "endpoint": "send-welcome-email",
  "endpoint_type": "HTTP",
  "trigger": "IMMEDIATE",
  "status": "ACTIVE",
  "version": 1,
  "idempotency_key": "order-1234-welcome-email",
  "input": { ... },
  "execution": {
    "execution_id": "exec_2b7c...",
    "status": "QUEUED",
    "created_at": "2026-03-15T10:00:00Z"
  },
  "created_at": "2026-03-15T10:00:00Z"
}
```

**Response — Cron (201):**
```json
{
  "job_id": "job_c72f...",
  "endpoint": "push-notification",
  "endpoint_type": "REDIS_STREAM",
  "trigger": "CRON",
  "status": "ACTIVE",
  "version": 1,
  "cron": "0 9 * * MON",
  "timezone": "Asia/Kolkata",
  "starts_at": "2026-03-16T00:00:00Z",
  "ends_at": null,
  "next_run_at": "2026-03-16T09:00:00+05:30",
  "input": { ... },
  "created_at": "2026-03-15T10:00:00Z"
}
```

**200 OK** if deduplicated (idempotency key hit).

**Job creation fields:**

| Field | Type | Required | Description |
|-------|------|:--------:|-------------|
| `endpoint` | string | ✓ | Name of a registered endpoint. |
| `trigger` | string | ✓ | `IMMEDIATE`, `DELAYED`, `CRON`. |
| `idempotency_key` | string | ✓* | *Required for IMMEDIATE and DELAYED. |
| `input` | object | | Validated against endpoint's payload spec. Static for CRON. |
| `run_at` | ISO 8601 | | Required for DELAYED. |
| `cron` | string | | Required for CRON. 5-field cron expression. |
| `timezone` | string | | Required for CRON. IANA timezone. |
| `starts_at` | ISO 8601 | | Optional for CRON. Default: now. |
| `ends_at` | ISO 8601 | | Optional for CRON. null = indefinite. |

**Update CRON job (`PUT /jobs/{job_id}`):**

Only for CRON jobs. Creates a new version, retires old. Returns `409 JOB_NOT_UPDATABLE` for one-shot jobs.

```json
{
  "cron": "0 10 * * MON",
  "timezone": "Asia/Kolkata",
  "input": { "report_type": "weekly", "include_charts": true }
}
```

Response includes `version: 2`, `previous_version_id`, and the old job is set to `status: RETIRED`.

**Cancel (`POST /jobs/{job_id}/cancel`):**

CRON: sets `status = RETIRED`, stops future executions. In-flight executions run to completion.
One-shot: cancels execution if PENDING/QUEUED. Returns 409 if RUNNING.

**Status (`GET /jobs/{job_id}/status`):**

Health overview for CRON jobs:
```json
{
  "job_id": "job_c72f...",
  "endpoint": "push-notification",
  "endpoint_type": "REDIS_STREAM",
  "trigger": "CRON",
  "health": "HEALTHY",
  "version": 1,
  "last_execution": {
    "execution_id": "exec_8f3a...",
    "status": "SUCCESS",
    "started_at": "2026-03-15T10:30:00Z",
    "completed_at": "2026-03-15T10:30:01Z",
    "attempt_number": 1
  },
  "active_executions": { "pending": 2, "running": 1, "total": 3 },
  "cron": {
    "expression": "0 9 * * MON",
    "next_run_at": "2026-03-16T09:00:00+05:30",
    "last_tick_at": "2026-03-09T09:00:00+05:30"
  },
  "stats": {
    "last_24h": {
      "total": 142, "succeeded": 139, "failed": 3,
      "avg_duration_ms": 340, "p99_duration_ms": 1200
    }
  }
}
```

Health values: `HEALTHY` (mostly succeeding), `DEGRADED` (elevated failures), `FAILING` (mostly failing), `IDLE` (no recent executions).

### Execution Endpoints (for later, but define the types now)

```
GET    /executions/{execution_id}                Get execution details
POST   /executions/{execution_id}/cancel         Cancel if PENDING/QUEUED
GET    /executions/{execution_id}/attempts       List attempts
GET    /executions/{execution_id}/logs           Get logs
GET    /jobs/{job_id}/executions                 List executions for a job
```

### All List Endpoints

All list endpoints support cursor-based pagination with `limit` (default 50, max 200) and `cursor` query params. Response shape: `{ "items": [...], "cursor": "..." }`.

### Error Responses

All errors:
```json
{
  "error": {
    "code": "JOB_NOT_FOUND",
    "message": "Job with ID 'job_xyz' does not exist.",
    "request_id": "req_9a8b..."
  }
}
```

Error codes:

| HTTP | Code | Description |
|:----:|------|-------------|
| 400 | `INVALID_REQUEST` | Malformed request body. |
| 401 | `UNAUTHORIZED` | Missing or invalid API key. |
| 404 | `PAYLOAD_SPEC_NOT_FOUND` | Payload spec not found. |
| 404 | `CONFIG_NOT_FOUND` | Config not found. |
| 404 | `SECRET_NOT_FOUND` | Secret not found. |
| 404 | `ENDPOINT_NOT_FOUND` | Endpoint not found. |
| 404 | `JOB_NOT_FOUND` | Job not found. |
| 404 | `EXECUTION_NOT_FOUND` | Execution not found. |
| 409 | `CONFLICT` | Resource has active dependents. |
| 409 | `JOB_NOT_UPDATABLE` | Can't update one-shot jobs. |
| 409 | `EXECUTION_NOT_CANCELLABLE` | Already running or completed. |
| 422 | `INVALID_CRON` | Invalid cron expression. |
| 422 | `INVALID_SCHEMA` | Invalid JSON Schema. |
| 422 | `INVALID_PAYLOAD_SPEC_REF` | Payload spec reference not found. |
| 422 | `INVALID_CONFIG_REF` | Config reference not found. |
| 422 | `INPUT_VALIDATION_FAILED` | Input doesn't match payload spec. |
| 422 | `TEMPLATE_RESOLUTION_FAILED` | Template variable unresolvable. |
| 429 | `RATE_LIMITED` | Too many requests. |
| 500 | `INTERNAL_ERROR` | Server error. |

---

## CockroachDB Schema

### Database Setup

```sql
CREATE DATABASE taskexecutor;
USE taskexecutor;

-- Single region for now. When adding regions later:
-- ALTER DATABASE taskexecutor SET PRIMARY REGION = 'ap-south-1';
-- ALTER DATABASE taskexecutor ADD REGION 'ap-south-2';
-- ALTER DATABASE taskexecutor SURVIVE ZONE FAILURE;
```

### Tables

#### payload_specs

```sql
CREATE TABLE payload_specs (
    name          STRING        NOT NULL,
    schema_json   JSONB         NOT NULL,
    created_at    TIMESTAMPTZ   NOT NULL DEFAULT now(),
    updated_at    TIMESTAMPTZ   NOT NULL DEFAULT now(),
    CONSTRAINT pk_payload_specs PRIMARY KEY (name)
);
```

#### configs

```sql
CREATE TABLE configs (
    name          STRING        NOT NULL,
    values_json   JSONB         NOT NULL,
    created_at    TIMESTAMPTZ   NOT NULL DEFAULT now(),
    updated_at    TIMESTAMPTZ   NOT NULL DEFAULT now(),
    CONSTRAINT pk_configs PRIMARY KEY (name)
);
```

#### secrets

```sql
CREATE TABLE secrets (
    name              STRING        NOT NULL,
    encrypted_value   BYTES         NOT NULL,
    created_at        TIMESTAMPTZ   NOT NULL DEFAULT now(),
    updated_at        TIMESTAMPTZ   NOT NULL DEFAULT now(),
    CONSTRAINT pk_secrets PRIMARY KEY (name)
);
```

#### endpoints

```sql
CREATE TABLE endpoints (
    name              STRING        NOT NULL,
    endpoint_type     STRING        NOT NULL,    -- HTTP, KAFKA, REDIS_STREAM
    payload_spec_ref  STRING,
    config_ref        STRING,
    spec              JSONB         NOT NULL,
    retry_policy      JSONB,
    created_at        TIMESTAMPTZ   NOT NULL DEFAULT now(),
    updated_at        TIMESTAMPTZ   NOT NULL DEFAULT now(),
    CONSTRAINT pk_endpoints PRIMARY KEY (name),
    CONSTRAINT fk_endpoints_payload_spec FOREIGN KEY (payload_spec_ref) REFERENCES payload_specs (name),
    CONSTRAINT fk_endpoints_config FOREIGN KEY (config_ref) REFERENCES configs (name),
    CONSTRAINT chk_endpoint_type CHECK (endpoint_type IN ('HTTP', 'KAFKA', 'REDIS_STREAM'))
);

CREATE INDEX idx_endpoints_type ON endpoints (endpoint_type);
```

#### jobs

```sql
CREATE TABLE jobs (
    job_id                STRING        NOT NULL DEFAULT gen_random_uuid()::STRING,
    crdb_region           STRING        NOT NULL DEFAULT 'default',
    endpoint              STRING        NOT NULL,
    endpoint_type         STRING        NOT NULL,
    trigger_type          STRING        NOT NULL,    -- IMMEDIATE, DELAYED, CRON
    status                STRING        NOT NULL DEFAULT 'ACTIVE',
    version               INT           NOT NULL DEFAULT 1,
    previous_version_id   STRING,
    replaced_by_id        STRING,
    idempotency_key       STRING,
    input                 JSONB,
    run_at                TIMESTAMPTZ,
    cron_expression       STRING,
    cron_timezone         STRING,
    cron_starts_at        TIMESTAMPTZ,
    cron_ends_at          TIMESTAMPTZ,
    cron_next_run_at      TIMESTAMPTZ,
    cron_last_tick_at     TIMESTAMPTZ,
    created_at            TIMESTAMPTZ   NOT NULL DEFAULT now(),
    retired_at            TIMESTAMPTZ,
    CONSTRAINT pk_jobs PRIMARY KEY (job_id),
    CONSTRAINT fk_jobs_endpoint FOREIGN KEY (endpoint) REFERENCES endpoints (name),
    CONSTRAINT chk_trigger_type CHECK (trigger_type IN ('IMMEDIATE', 'DELAYED', 'CRON')),
    CONSTRAINT chk_status CHECK (status IN ('ACTIVE', 'RETIRED')),
    CONSTRAINT chk_endpoint_type CHECK (endpoint_type IN ('HTTP', 'KAFKA', 'REDIS_STREAM'))
);

CREATE UNIQUE INDEX idx_jobs_idempotency
    ON jobs (endpoint, idempotency_key)
    WHERE idempotency_key IS NOT NULL;

CREATE INDEX idx_jobs_cron_due
    ON jobs (cron_next_run_at)
    WHERE trigger_type = 'CRON' AND status = 'ACTIVE';

CREATE INDEX idx_jobs_endpoint
    ON jobs (endpoint, created_at DESC);

CREATE INDEX idx_jobs_status
    ON jobs (status, created_at DESC);
```

Note: `crdb_region` column is present but set to `'default'` in single-region mode. When multi-region is enabled, this becomes `crdb_internal_region` type with `gateway_region()` default and the table gets `ALTER TABLE jobs SET LOCALITY REGIONAL BY ROW`.

#### executions

```sql
CREATE TABLE executions (
    execution_id    STRING        NOT NULL DEFAULT gen_random_uuid()::STRING,
    crdb_region     STRING        NOT NULL DEFAULT 'default',
    job_id          STRING        NOT NULL,
    endpoint        STRING        NOT NULL,
    endpoint_type   STRING        NOT NULL,
    idempotency_key STRING,
    status          STRING        NOT NULL DEFAULT 'PENDING',
    input           JSONB,
    output          JSONB,
    attempt_count   INT           NOT NULL DEFAULT 0,
    max_attempts    INT           NOT NULL DEFAULT 1,
    worker_id       STRING,
    run_at          TIMESTAMPTZ   NOT NULL DEFAULT now(),
    started_at      TIMESTAMPTZ,
    completed_at    TIMESTAMPTZ,
    duration_ms     INT,
    created_at      TIMESTAMPTZ   NOT NULL DEFAULT now(),
    CONSTRAINT pk_executions PRIMARY KEY (execution_id),
    CONSTRAINT fk_executions_job FOREIGN KEY (job_id) REFERENCES jobs (job_id),
    CONSTRAINT chk_exec_status CHECK (status IN (
        'PENDING', 'QUEUED', 'RUNNING', 'RETRYING', 'SUCCESS', 'FAILED', 'CANCELLED'
    ))
);

-- THE hot-path query index
CREATE INDEX idx_executions_pickup
    ON executions (status, run_at ASC)
    WHERE status IN ('QUEUED', 'RETRYING');

CREATE UNIQUE INDEX idx_executions_cron_dedup
    ON executions (job_id, idempotency_key)
    WHERE idempotency_key IS NOT NULL;

CREATE INDEX idx_executions_by_job
    ON executions (job_id, created_at DESC);

CREATE INDEX idx_executions_running
    ON executions (status, started_at)
    WHERE status = 'RUNNING';
```

#### attempts

```sql
CREATE TABLE attempts (
    attempt_id      STRING        NOT NULL DEFAULT gen_random_uuid()::STRING,
    crdb_region     STRING        NOT NULL DEFAULT 'default',
    execution_id    STRING        NOT NULL,
    attempt_number  INT           NOT NULL,
    status          STRING        NOT NULL,    -- SUCCESS, FAILED
    started_at      TIMESTAMPTZ   NOT NULL,
    completed_at    TIMESTAMPTZ,
    duration_ms     INT,
    output          JSONB,
    error           JSONB,        -- { type, message }
    created_at      TIMESTAMPTZ   NOT NULL DEFAULT now(),
    CONSTRAINT pk_attempts PRIMARY KEY (attempt_id),
    CONSTRAINT fk_attempts_execution FOREIGN KEY (execution_id) REFERENCES executions (execution_id),
    CONSTRAINT uq_attempts_exec_number UNIQUE (execution_id, attempt_number),
    CONSTRAINT chk_attempt_status CHECK (status IN ('SUCCESS', 'FAILED'))
);

CREATE INDEX idx_attempts_by_execution
    ON attempts (execution_id, attempt_number ASC);
```

Error shapes by transport:

HTTP: `{ "type": "TIMEOUT", "message": "..." }` or `{ "type": "HTTP_ERROR", "status_code": 503, "message": "..." }` or `{ "type": "CONNECTION_ERROR", "message": "..." }`

Kafka: `{ "type": "BROKER_ERROR", "message": "..." }` or `{ "type": "TIMEOUT", "message": "..." }`

Redis: `{ "type": "CONNECTION_ERROR", "message": "..." }` or `{ "type": "TIMEOUT", "message": "..." }` or `{ "type": "STREAM_ERROR", "message": "..." }`

Output shapes by transport:

HTTP: `{ "status_code": 200, "body": "OK" }`
Kafka: `{ "partition": 3, "offset": 12847 }`
Redis: `{ "message_id": "1710499801234-0", "stream": "notifications:outbound" }`

#### execution_logs

```sql
CREATE TABLE execution_logs (
    log_id          STRING        NOT NULL DEFAULT gen_random_uuid()::STRING,
    crdb_region     STRING        NOT NULL DEFAULT 'default',
    execution_id    STRING        NOT NULL,
    attempt_number  INT           NOT NULL,
    level           STRING        NOT NULL,    -- DEBUG, INFO, WARN, ERROR
    message         STRING        NOT NULL,
    logged_at       TIMESTAMPTZ   NOT NULL DEFAULT now(),
    CONSTRAINT pk_execution_logs PRIMARY KEY (log_id),
    CONSTRAINT fk_logs_execution FOREIGN KEY (execution_id) REFERENCES executions (execution_id),
    CONSTRAINT chk_log_level CHECK (level IN ('DEBUG', 'INFO', 'WARN', 'ERROR'))
);

CREATE INDEX idx_logs_by_execution
    ON execution_logs (execution_id, logged_at ASC);

CREATE INDEX idx_logs_by_attempt
    ON execution_logs (execution_id, attempt_number, logged_at ASC);
```

#### region_heartbeats and region_status (stub for later)

```sql
CREATE TABLE region_heartbeats (
    region        STRING        NOT NULL,
    component     STRING        NOT NULL,
    last_beat_at  TIMESTAMPTZ   NOT NULL DEFAULT now(),
    status        STRING        NOT NULL DEFAULT 'ALIVE',
    metadata      JSONB,
    CONSTRAINT pk_region_heartbeats PRIMARY KEY (region, component)
);

CREATE TABLE region_status (
    region        STRING        NOT NULL,
    alive         BOOL          NOT NULL DEFAULT true,
    failed_at     TIMESTAMPTZ,
    adopted_by    STRING,
    updated_at    TIMESTAMPTZ   NOT NULL DEFAULT now(),
    CONSTRAINT pk_region_status PRIMARY KEY (region)
);

INSERT INTO region_status (region, alive, updated_at) VALUES ('default', true, now());
```

---

## Key Queries

### Worker Pickup (hot path)

```sql
UPDATE executions
SET status = 'RUNNING',
    worker_id = $1,
    started_at = now(),
    attempt_count = attempt_count + 1
WHERE execution_id = (
    SELECT execution_id
    FROM executions
    WHERE status IN ('QUEUED', 'RETRYING')
      AND run_at <= now()
    ORDER BY run_at ASC
    LIMIT 1
    FOR UPDATE SKIP LOCKED
)
RETURNING execution_id, job_id, endpoint, endpoint_type, input, attempt_count, max_attempts;
```

### Job Creation — Immediate

```sql
BEGIN;
INSERT INTO jobs (endpoint, endpoint_type, trigger_type, idempotency_key, input)
VALUES ($1, $2, 'IMMEDIATE', $3, $4)
RETURNING job_id;

INSERT INTO executions (job_id, endpoint, endpoint_type, idempotency_key, status, run_at, input, max_attempts)
VALUES ($5, $1, $2, $3, 'QUEUED', now(), $4, $6)
RETURNING execution_id, status, created_at;
COMMIT;
```

### Job Creation — Delayed

```sql
BEGIN;
INSERT INTO jobs (endpoint, endpoint_type, trigger_type, idempotency_key, input, run_at)
VALUES ($1, $2, 'DELAYED', $3, $4, $5)
RETURNING job_id;

INSERT INTO executions (job_id, endpoint, endpoint_type, idempotency_key, status, run_at, input, max_attempts)
VALUES ($6, $1, $2, $3, 'PENDING', $5, $4, $7)
RETURNING execution_id, status, created_at;
COMMIT;
```

### CRON Tick Materialization

```sql
-- Find due CRON jobs
SELECT job_id, endpoint, endpoint_type, input, cron_expression, cron_timezone, cron_next_run_at
FROM jobs
WHERE trigger_type = 'CRON' AND status = 'ACTIVE'
  AND cron_next_run_at <= now()
  AND (cron_ends_at IS NULL OR cron_ends_at > now())
LIMIT 100;

-- For each: create execution (idempotent) + advance tick
INSERT INTO executions (job_id, endpoint, endpoint_type, idempotency_key, status, input, run_at, max_attempts)
VALUES ($1, $2, $3, 'cron_' || $1 || '_' || extract(epoch from $4)::TEXT, 'QUEUED', $5, $4, $6)
ON CONFLICT (job_id, idempotency_key) DO NOTHING;

UPDATE jobs SET cron_next_run_at = $7, cron_last_tick_at = $4
WHERE job_id = $1 AND cron_next_run_at = $4;  -- CAS
```

### Delayed Job Promotion

```sql
UPDATE executions SET status = 'QUEUED'
WHERE status = 'PENDING' AND run_at <= now();
```

### Execution Completion

```sql
UPDATE executions
SET status = 'SUCCESS', output = $2, completed_at = now(),
    duration_ms = extract(epoch from (now() - started_at))::INT * 1000
WHERE execution_id = $1 AND status = 'RUNNING';
```

### Execution Retry

```sql
UPDATE executions
SET status = CASE WHEN attempt_count >= max_attempts THEN 'FAILED' ELSE 'RETRYING' END,
    run_at = CASE WHEN attempt_count >= max_attempts THEN run_at
             ELSE now() + ($2 * interval '1 millisecond') END,
    worker_id = NULL
WHERE execution_id = $1 AND status = 'RUNNING';
```

### Stuck Execution Recovery

```sql
UPDATE executions
SET status = CASE WHEN attempt_count >= max_attempts THEN 'FAILED' ELSE 'RETRYING' END,
    worker_id = NULL, run_at = now()
WHERE status = 'RUNNING' AND started_at < now() - interval '5 minutes';
```

---

## Worker Architecture

Runtime: Rust + tokio. Generic worker pool — every worker handles all three transports.

### Process Structure

```
Worker Process (tokio runtime)
├── Poller Loop (single task)
│   ├── Acquires semaphore permit (backpressure)
│   ├── Claims execution via SKIP LOCKED
│   ├── Spawns tokio task for execution pipeline
│   └── Sleeps poll_interval only when queue is empty
│
├── N Concurrent Execution Pipelines (spawned tasks)
│   ├── 1. Resolve templates (config cache + secret cache + input)
│   ├── 2. Dispatch (HTTP / Kafka / Redis Stream)
│   ├── 3. Record attempt (INSERT into attempts)
│   ├── 4. Finalize (UPDATE execution: SUCCESS / RETRYING / FAILED)
│   └── 5. Release semaphore permit
│
├── Background: Heartbeat writer (every 5s) — stub for now
├── Background: Region status refresher (every 10s) — stub for now
└── Background: Metrics reporter (every 15s)
```

### Poller Loop (pseudocode)

```rust
let semaphore = Arc::new(Semaphore::new(config.max_concurrent_jobs)); // 50
let poll_interval = Duration::from_millis(config.poll_interval_ms);   // 200

loop {
    let permit = semaphore.clone().acquire_owned().await?;
    let execution = db.claim_execution(worker_id).await;

    match execution {
        Some(exec) => {
            tokio::spawn(async move {
                process_execution(exec).await;
                drop(permit);
            });
        }
        None => {
            drop(permit);
            tokio::time::sleep(poll_interval).await;
        }
    }
}
```

### Execution Pipeline

1. **Resolve templates**: load endpoint config from cache (DashMap, 60s TTL), load secrets from cache (DashMap, 300s TTL), resolve `{{config.*}}` and `{{secret.*}}`, then `{{input.*}}`. Fail with TEMPLATE_RESOLUTION_FAILED if any variable unresolvable (no retry — it'll fail the same way).

2. **Dispatch**: match on `endpoint_type`, call appropriate async dispatcher:
   - HTTP: `reqwest::Client` (shared, keep-alive pool). Check response status against `expected_status_codes`.
   - Kafka: `rdkafka::FutureProducer` (shared). Produce message, await broker ack.
   - Redis Stream: `redis::aio::ConnectionManager` (pooled). XADD with optional MAXLEN trimming.

3. **Record attempt**: INSERT into attempts table with attempt_number, status, duration, output/error.

4. **Finalize execution**:
   - Success → `UPDATE status = 'SUCCESS', output = ..., completed_at = now()`
   - Failure + retries remaining → `UPDATE status = 'RETRYING', run_at = now() + backoff, worker_id = NULL`
   - Failure + retries exhausted → `UPDATE status = 'FAILED', completed_at = now()`

### Backoff Computation

```rust
fn compute_backoff(policy: &RetryPolicy, attempt: i32) -> i64 {
    let delay = match policy.backoff {
        Fixed => policy.initial_delay_ms,
        Linear => policy.initial_delay_ms * attempt as i64,
        Exponential => policy.initial_delay_ms * 2_i64.pow((attempt - 1) as u32),
    };
    // Add ±25% jitter to prevent thundering herd
    let jitter = rand::thread_rng().gen_range(-(delay/4)..=(delay/4));
    (delay + jitter).clamp(0, policy.max_delay_ms)
}
```

### Graceful Shutdown

On SIGTERM: stop poller (no new claims), wait up to 30s for in-flight tasks, write final heartbeat with status 'DEAD'. Anything still running stays in RUNNING state — the stuck reclaimer will reset it.

### Shared State

- DB pool: `sqlx::PgPool` or `deadpool-postgres`
- HTTP client: `reqwest::Client` (reused, keep-alive)
- Kafka producer: `rdkafka::FutureProducer` (shared across tasks)
- Redis pool: `redis::aio::ConnectionManager`
- Config cache: `DashMap<String, (Config, Instant)>` with 60s TTL
- Secret cache: `DashMap<String, (DecryptedSecret, Instant)>` with 300s TTL
- Region status: `AtomicBool` per region (stub: always true for now)

---

## Scheduler Architecture

Runtime: Rust + tokio. Three independent loops. Safe to run multiple instances — all loops are idempotent.

### Process Structure

```
Scheduler Process (tokio runtime)
├── Loop 1: CRON Tick Materializer (every 1s)
│   ├── SELECT CRON jobs where cron_next_run_at <= now()
│   ├── For each: INSERT execution with idempotency key (ON CONFLICT DO NOTHING)
│   ├── Advance cron_next_run_at via CAS (WHERE cron_next_run_at = current_tick)
│   └── Catches up on missed ticks by computing next from current tick, not now()
│
├── Loop 2: Delayed Job Promoter (every 500ms)
│   └── UPDATE executions SET status = 'QUEUED' WHERE status = 'PENDING' AND run_at <= now()
│
├── Loop 3: Stuck Execution Reclaimer (every 30s)
│   └── UPDATE stuck RUNNING executions to RETRYING or FAILED (based on max_attempts)
│
├── Background: Heartbeat writer (every 5s) — stub for now
└── Background: Region status refresher (every 10s) — stub for now
```

### CRON Catch-Up Behavior

Compute `next_run_at` from the current tick, not `now()`. If the scheduler was down and missed ticks, it materializes all missed ones sequentially:

```
CRON job: every minute. Scheduler was down from 09:00 to 09:10.
Iteration 1: next_run_at = 09:00 (due). Creates exec. Advances to 09:01.
Iteration 2: next_run_at = 09:01 (due). Creates exec. Advances to 09:02.
...
Iteration 11: next_run_at = 09:10 (due). Creates exec. Advances to 09:11.
Iteration 12: next_run_at = 09:11 (not yet due). Sleeps.
```

### Multiple Scheduler Safety

| Loop | Idempotency mechanism |
|------|----------------------|
| CRON materializer | `ON CONFLICT DO NOTHING` on execution + CAS on `cron_next_run_at` |
| Delayed promoter | `UPDATE WHERE status = 'PENDING'` — first wins, second finds nothing |
| Stuck reclaimer | `UPDATE WHERE status = 'RUNNING'` — same, first wins |

No leader election needed.

---

## Configuration

### Worker config

| Parameter | Default | Env var |
|-----------|---------|---------|
| `max_concurrent_jobs` | 50 | `TE_WORKER_MAX_CONCURRENT` |
| `poll_interval_ms` | 200 | `TE_WORKER_POLL_INTERVAL_MS` |
| `config_cache_ttl_secs` | 60 | `TE_CONFIG_CACHE_TTL_SEC` |
| `secret_cache_ttl_secs` | 300 | `TE_SECRET_CACHE_TTL_SEC` |
| `shutdown_timeout_secs` | 30 | `TE_WORKER_SHUTDOWN_TIMEOUT_SEC` |
| `db_pool_size` | 10 | `TE_DB_POOL_SIZE` |

### Scheduler config

| Parameter | Default | Env var |
|-----------|---------|---------|
| `cron_tick_interval_secs` | 1 | `TE_CRON_TICK_INTERVAL_SEC` |
| `cron_batch_size` | 100 | `TE_CRON_BATCH_SIZE` |
| `promote_interval_ms` | 500 | `TE_PROMOTE_INTERVAL_MS` |
| `reclaim_interval_secs` | 30 | `TE_RECLAIM_INTERVAL_SEC` |
| `stuck_execution_timeout_secs` | 300 | `TE_STUCK_EXECUTION_TIMEOUT_SEC` |

### API server config

| Parameter | Default | Env var |
|-----------|---------|---------|
| `listen_addr` | `0.0.0.0:8080` | `TE_LISTEN_ADDR` |
| `db_url` | required | `TE_DATABASE_URL` |
| `db_pool_size` | 20 | `TE_DB_POOL_SIZE` |

---

## Crate Recommendations

| Purpose | Crate |
|---------|-------|
| Async runtime | `tokio` |
| HTTP framework | `axum` |
| DB driver | `sqlx` (with `postgres` feature) or `tokio-postgres` + `deadpool-postgres` |
| HTTP client | `reqwest` |
| Kafka | `rdkafka` |
| Redis | `redis` (with `tokio-comp` feature) |
| JSON Schema validation | `jsonschema` |
| Cron parsing | `cron` |
| Serialization | `serde`, `serde_json` |
| Config | `config` or env-based with `envy` |
| Concurrent hashmap | `dashmap` |
| UUID | `uuid` |
| Time | `chrono` |
| Tracing/logging | `tracing`, `tracing-subscriber` |
| Graceful shutdown | `tokio::signal` |
| Template resolution | Custom (simple string replacement on `{{...}}` patterns) |

---

## Project Structure

```
task-executor/
├── Cargo.toml
├── migrations/
│   └── 001_initial.sql              # All CREATE TABLE statements
├── src/
│   ├── main.rs                      # Entry point: starts API, worker, scheduler
│   ├── config.rs                    # App configuration from env vars
│   ├── error.rs                     # Error types and API error responses
│   ├── db/
│   │   ├── mod.rs
│   │   └── pool.rs                  # Connection pool setup
│   ├── api/
│   │   ├── mod.rs                   # axum Router setup
│   │   ├── payload_specs.rs         # CRUD handlers
│   │   ├── configs.rs               # CRUD handlers
│   │   ├── secrets.rs               # CRUD handlers
│   │   ├── endpoints.rs             # CRUD handlers
│   │   ├── jobs.rs                  # Create, get, update, cancel, status, versions
│   │   └── executions.rs            # Get, cancel, list attempts, list logs
│   ├── models/
│   │   ├── mod.rs
│   │   ├── payload_spec.rs
│   │   ├── config.rs
│   │   ├── secret.rs
│   │   ├── endpoint.rs              # Endpoint + HttpSpec + KafkaSpec + RedisStreamSpec
│   │   ├── job.rs
│   │   ├── execution.rs
│   │   └── attempt.rs
│   ├── worker/
│   │   ├── mod.rs                   # Worker process entry point
│   │   ├── poller.rs                # Poller loop with semaphore
│   │   ├── pipeline.rs              # Execution pipeline (resolve → dispatch → record → finalize)
│   │   ├── dispatcher/
│   │   │   ├── mod.rs               # Dispatch trait + match on endpoint_type
│   │   │   ├── http.rs              # reqwest-based HTTP dispatcher
│   │   │   ├── kafka.rs             # rdkafka-based Kafka dispatcher
│   │   │   └── redis_stream.rs      # redis-rs-based Redis Stream dispatcher
│   │   ├── template.rs              # Template resolution engine ({{input.*}}, {{config.*}}, {{secret.*}})
│   │   └── backoff.rs               # Backoff computation with jitter
│   ├── scheduler/
│   │   ├── mod.rs                   # Scheduler process entry point
│   │   ├── cron_materializer.rs     # Loop 1: CRON tick materialization
│   │   ├── delayed_promoter.rs      # Loop 2: PENDING → QUEUED promotion
│   │   └── stuck_reclaimer.rs       # Loop 3: Stuck execution recovery
│   ├── health/
│   │   ├── mod.rs                   # Stub: heartbeat writer, region status
│   │   ├── heartbeat.rs             # Stub: writes to region_heartbeats
│   │   └── region.rs                # Stub: reads region_status, always returns all-alive
│   └── cache/
│       ├── mod.rs
│       ├── config_cache.rs          # DashMap with TTL for configs
│       └── secret_cache.rs          # DashMap with TTL for decrypted secrets
```

---

## Process Topology (Single Region)

```
├── API Server (1-2 instances)
│   └── axum HTTP server
│       Handles all REST endpoints
│       On job creation: INSERT job + execution in transaction
│
├── Worker (2-3 instances)
│   └── tokio runtime
│       Poller + N concurrent execution pipelines
│       Shared HTTP client, Kafka producer, Redis pool
│
├── Scheduler (2 instances for redundancy)
│   └── tokio runtime
│       CRON materializer + delayed promoter + stuck reclaimer
│
└── CockroachDB (3 nodes, spread across AZs)
```

---

## End-to-End Flows

### Immediate Job

```
1. POST /jobs { trigger: IMMEDIATE, endpoint: "send-welcome-email", ... }
2. API: BEGIN → INSERT job → INSERT execution (QUEUED) → COMMIT → return 201
3. Worker poller (~200ms): SKIP LOCKED claims execution → spawns task
4. Pipeline: cache-hit config → resolve templates → HTTP POST → 200 OK
5. Record attempt (SUCCESS) → UPDATE execution (SUCCESS)
6. Done. ~300ms total.
```

### Delayed Job

```
1. POST /jobs { trigger: DELAYED, run_at: "18:00:00Z", ... }
2. API: INSERT job + execution (PENDING, run_at = 18:00)
3. Scheduler promoter (at 18:00, within 500ms): UPDATE → QUEUED
4. Worker poller (~200ms): claims → dispatches
5. Done. Fires within ~700ms of run_at.
```

### CRON Job

```
1. POST /jobs { trigger: CRON, cron: "0 9 * * MON", ... }
2. API: INSERT job (ACTIVE, cron_next_run_at = next Monday 09:00)
3. Every Monday at 09:00:
   a. Scheduler materializer (~1s): INSERT execution (QUEUED) with idempotency key
   b. Advance cron_next_run_at
   c. Worker picks up and dispatches
4. Repeats until POST /jobs/{id}/cancel
```

---

## What to Stub (Activate Later for Multi-Region)

1. **Heartbeat writer**: write to `region_heartbeats` every 5s. For now, write with `region = 'default'` and don't act on it.

2. **Region status refresher**: read `region_status` every 10s. For now, always return `active_regions = vec!["default"]`.

3. **Region-aware queries**: the `crdb_region` column exists on jobs, executions, attempts, logs. For now it's always `'default'`. Queries don't need `WHERE crdb_region = ANY($1)` — but structure the code so adding that filter is trivial (e.g., a `RegionFilter` parameter that's currently a no-op).

4. **Health evaluator process**: don't build it yet. Just define the module structure (`health/evaluator.rs`) with a `todo!()` or empty loop.

---

## Important Implementation Notes

1. **Idempotency on job creation**: the unique index on `(endpoint, idempotency_key)` handles dedup. On conflict, return the existing job with `200 OK` instead of `201 Created`. Use `INSERT ... ON CONFLICT DO NOTHING RETURNING ...` or catch the unique violation error.

2. **CRON next tick CAS**: the `UPDATE jobs SET cron_next_run_at = $new WHERE cron_next_run_at = $current` pattern prevents double-ticking when multiple schedulers run. If affected rows = 0, another scheduler already advanced it.

3. **Template resolution**: implement as simple recursive string replacement on `{{...}}` patterns. Walk the JSON tree, replace any string value containing `{{input.X}}` with the corresponding value from the input object. Same for `{{config.X}}` and `{{secret.X}}`. Support nested paths like `{{input.user.name}}`.

4. **Secret encryption**: secrets should be encrypted at rest in the DB. Use a symmetric key (from env var `TE_SECRET_ENCRYPTION_KEY`). Encrypt on write, decrypt on read into the cache.

5. **JSON Schema validation**: use the `jsonschema` crate to validate job input against the endpoint's payload spec before creating the execution.

6. **All list endpoints**: support cursor-based pagination. The cursor is an opaque base64-encoded string containing the last seen `created_at` + `id` for stable pagination.
