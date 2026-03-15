# Kronos — Remote Invoker Service API Specification

**Version:** 5.0.0
**Date:** March 15, 2026

---

## Overview

Kronos is a distributed remote invocation engine. It provides durable, exactly-once, retriable invocation of remote endpoints over HTTP, Kafka, and Redis Streams — with support for immediate, delayed, and recurring triggers.

**Base URL:** `https://api.kronos.io/v1`

### How It Works

| Step | What you do | API |
|------|------------|-----|
| **1. Setup** | Create configs, secrets, and payload specs | `/configs`, `/payload-specs`, `/secrets` |
| **2. Register** | Register a remote endpoint | `POST /endpoints` |
| **3. Invoke** | Fire the endpoint — now, later, or on a schedule | `POST /jobs` |

### Conceptual Model

| JS Primitive | Kronos | Trigger |
|---|---|---|
| `setTimeout(fn, 0)` | `POST /jobs { trigger: IMMEDIATE }` | Fire once, now |
| `setTimeout(fn, delay)` | `POST /jobs { trigger: DELAYED }` | Fire once, later |
| `setInterval(fn, interval)` | `POST /jobs { trigger: CRON }` | Fire repeatedly |
| `clearTimeout` / `clearInterval` | `POST /jobs/{id}/cancel` | Stop |

**What the platform adds:** durability, distributed execution, retry with backoff, exactly-once guarantees, and full observability.

---

## Core Concepts

### Domain Model

| Entity | Description |
|--------|-------------|
| **Payload Spec** | A JSON Schema defining the input contract for an endpoint. Validated at job creation time. |
| **Config** | A key-value object holding static variables (base URLs, broker addresses, feature flags). Referenced by endpoints, resolved at execution runtime. |
| **Secret** | A sensitive value (API key, credential). Referenced via `{{secret.*}}`. Resolved at runtime, never exposed in responses. |
| **Endpoint** | A registered remote target — HTTP URL, Kafka topic, or Redis Stream. Defines the transport, target details, retry policy, and references to payload specs and configs. Created once, invoked by many jobs. |
| **Job** | An invocation of an endpoint. Creating a job triggers execution. One-shot jobs (`IMMEDIATE`, `DELAYED`) fire once. Persistent jobs (`CRON`) generate executions on a schedule until cancelled. |
| **Execution** | A single invocation made by the system. Each job fire produces one execution. *(API deferred to later)* |
| **Attempt** | A single try within an execution. Failed attempts retry per the endpoint's retry policy. *(API deferred to later)* |

### Template Resolution

Endpoint targets use template variables resolved from three namespaces:

| Namespace | Source | Resolved when | Per-execution? |
|---|---|---|---|
| `{{input.*}}` | Job input | Execution runtime | Yes |
| `{{config.*}}` | Endpoint's referenced config | Execution runtime | No |
| `{{secret.*}}` | Secret store | Execution runtime | No |

### Deduplication

Single mechanism: **unique constraint on `(endpoint, idempotency_key)`**.

| Trigger | Key provided by | Example |
|---------|----------------|---------|
| `IMMEDIATE` / `DELAYED` | Client | `order-1234-welcome-email` |
| `CRON` | System | `job_{id}_{epoch_ms}` |

Duplicate requests return the existing entity with `200 OK` instead of `201 Created`.

### Immutability (Jobs)

Jobs are **immutable**. One-shot jobs fire and complete. `CRON` jobs are updated by creating a new version and retiring the old one, linked via `previous_version_id`.

```
job_abc (v1, ACTIVE)
    └──→ PUT /jobs/job_abc
            └──→ job_def (v2, ACTIVE)  ← previous_version_id: job_abc
                     job_abc (v1, RETIRED)
```

---

## Authentication

```
Authorization: Bearer <api_key>
```

---

## Step 1 — Setup

### Payload Specs

Payload specs define the input contract for job invocations. Stored as JSON Schema. When an endpoint references a payload spec, all job input is validated against it before execution.

#### Create a Payload Spec

```
POST /payload-specs
```

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

**Response — `201 Created`:**

```json
{
  "name": "send-welcome-email-input",
  "schema": { ... },
  "created_at": "2026-03-15T10:00:00Z",
  "updated_at": "2026-03-15T10:00:00Z"
}
```

#### Other Payload Spec Endpoints

```
GET    /payload-specs              List all payload specs
GET    /payload-specs/{name}       Get a payload spec
PUT    /payload-specs/{name}       Update a payload spec
DELETE /payload-specs/{name}       Delete (fails if endpoints reference it)
```

> **Note:** Updating a payload spec does not affect running executions. The updated spec applies to future job creations only.

---

### Configs

Configs hold static variables used across executions of an endpoint. Values are available in endpoint targets as `{{config.*}}`.

Configs are also used to store transport connection details (Kafka broker addresses, Redis connection URIs, etc.).

#### Create a Config

```
POST /configs
```

```json
{
  "name": "email-service",
  "values": {
    "api_base_url": "https://api.myapp.com",
    "sender": "noreply@myapp.com",
    "max_retries": 3
  }
}
```

**Example — Kafka connection config:**

```json
{
  "name": "kafka-cluster-prod",
  "values": {
    "bootstrap_servers": "broker1:9092,broker2:9092,broker3:9092",
    "topic": "user-events",
    "acks": "all"
  }
}
```

**Example — Redis connection config:**

```json
{
  "name": "redis-streams-prod",
  "values": {
    "uri": "redis://redis-cluster:6379",
    "stream": "notification-stream",
    "max_len": 10000
  }
}
```

**Response — `201 Created`:**

```json
{
  "name": "email-service",
  "values": { ... },
  "created_at": "2026-03-15T10:00:00Z",
  "updated_at": "2026-03-15T10:00:00Z"
}
```

#### Other Config Endpoints

```
GET    /configs              List all configs
GET    /configs/{name}       Get a config
PUT    /configs/{name}       Update a config
DELETE /configs/{name}       Delete (fails if endpoints reference it)
```

> **Note:** Config updates take effect for future executions. In-flight executions use the config snapshot from when they started.

---

### Secrets

Secrets hold sensitive values. Referenced in endpoint targets via `{{secret.*}}`. Values are write-only — never returned in API responses.

#### Create a Secret

```
POST /secrets
```

```json
{
  "name": "email_api_key",
  "value": "sk-live-abc123..."
}
```

**Response — `201 Created`:**

```json
{
  "name": "email_api_key",
  "created_at": "2026-03-15T10:00:00Z",
  "updated_at": "2026-03-15T10:00:00Z"
}
```

Note: `value` is never returned.

#### Other Secret Endpoints

```
GET    /secrets              List all secrets (names only, no values)
GET    /secrets/{name}       Get secret metadata (no value)
PUT    /secrets/{name}       Rotate / update a secret value
DELETE /secrets/{name}       Delete (fails if endpoints reference it)
```

---

## Step 2 — Register

### `POST /endpoints`

Register a remote endpoint. Defines the transport target, success criteria, retry behavior, and references to payload specs and configs.

#### HTTP Endpoint

```json
{
  "name": "send-welcome-email",
  "payload_spec": "send-welcome-email-input",
  "config": "email-service",
  "target": {
    "type": "HTTP",
    "url": "{{config.api_base_url}}/emails/welcome",
    "method": "POST",
    "headers": {
      "Authorization": "Bearer {{secret.email_api_key}}"
    },
    "body_template": {
      "user_id": "{{input.user_id}}",
      "sender": "{{config.sender}}"
    },
    "timeout_ms": 5000,
    "success_criteria": {
      "status_codes": [200, 201, 202, 204]
    }
  },
  "retry_policy": {
    "max_attempts": 3,
    "backoff": "exponential",
    "initial_delay_ms": 1000,
    "max_delay_ms": 30000,
    "retry_on": {
      "status_codes": [500, 502, 503, 504],
      "on_timeout": true
    }
  }
}
```

#### Kafka Endpoint

```json
{
  "name": "publish-user-event",
  "payload_spec": "user-event-input",
  "config": "kafka-cluster-prod",
  "target": {
    "type": "KAFKA",
    "bootstrap_servers": "{{config.bootstrap_servers}}",
    "topic": "{{config.topic}}",
    "acks": "{{config.acks}}",
    "partition_key": "{{input.user_id}}",
    "headers": {
      "x-correlation-id": "{{input.trace_id}}",
      "x-source": "kronos"
    },
    "message_template": {
      "event": "user_created",
      "user_id": "{{input.user_id}}",
      "timestamp": "{{input.created_at}}"
    },
    "success_criteria": {
      "require_ack": true
    }
  },
  "retry_policy": {
    "max_attempts": 5,
    "backoff": "exponential",
    "initial_delay_ms": 500,
    "max_delay_ms": 15000,
    "retry_on": {
      "on_broker_error": true,
      "on_timeout": true
    }
  }
}
```

#### Redis Streams Endpoint

```json
{
  "name": "enqueue-notification",
  "payload_spec": "notification-input",
  "config": "redis-streams-prod",
  "target": {
    "type": "REDIS_STREAM",
    "uri": "{{config.uri}}",
    "stream": "{{config.stream}}",
    "max_len": "{{config.max_len}}",
    "message_template": {
      "type": "push_notification",
      "user_id": "{{input.user_id}}",
      "title": "{{input.title}}",
      "body": "{{input.body}}"
    },
    "success_criteria": {
      "require_ack": true
    }
  },
  "retry_policy": {
    "max_attempts": 3,
    "backoff": "fixed",
    "initial_delay_ms": 200,
    "max_delay_ms": 2000,
    "retry_on": {
      "on_connection_error": true,
      "on_timeout": true
    }
  }
}
```

**Response — `201 Created`:**

```json
{
  "name": "send-welcome-email",
  "payload_spec": "send-welcome-email-input",
  "config": "email-service",
  "target": { ... },
  "retry_policy": { ... },
  "created_at": "2026-03-15T10:00:00Z",
  "updated_at": "2026-03-15T10:00:00Z"
}
```

> **Validation:** `payload_spec` and `config` references are verified at registration time. Invalid references → `422`.

### Other Endpoint Methods

```
GET    /endpoints              List all endpoints
GET    /endpoints/{name}       Get an endpoint
PUT    /endpoints/{name}       Update (applies to future jobs only)
DELETE /endpoints/{name}       Delete (fails if active jobs reference it)
```

### Endpoint — Field Reference

| Field | Type | Required | Description |
|-------|------|:--------:|-------------|
| `name` | string | ✓ | Unique identifier. URL-safe (lowercase alphanumeric, hyphens). |
| `payload_spec` | string | | Name of a registered payload spec. Enables input validation. |
| `config` | string | | Name of a registered config. Values available as `{{config.*}}`. |
| `target` | object | ✓ | Transport-specific target definition. See below. |
| `retry_policy` | object | | Retry behavior on failure. Transport-aware. See below. |

#### `target` — HTTP

| Field | Type | Required | Description |
|-------|------|:--------:|-------------|
| `type` | string | ✓ | `"HTTP"` |
| `url` | string | ✓ | Target URL. Supports `{{config.*}}`, `{{secret.*}}`. |
| `method` | string | ✓ | `GET`, `POST`, `PUT`, `PATCH`, `DELETE`. |
| `headers` | map | | Key-value pairs. Supports `{{config.*}}`, `{{secret.*}}`. |
| `body_template` | object | | JSON body. Supports `{{input.*}}`, `{{config.*}}`, `{{secret.*}}`. |
| `timeout_ms` | integer | ✓ | Request timeout in milliseconds. |
| `success_criteria` | object | | See below. |

##### `success_criteria` — HTTP

| Field | Type | Default | Description |
|-------|------|:-------:|-------------|
| `status_codes` | integer[] | `[200, 201, 202, 204]` | HTTP status codes treated as success. |

#### `target` — Kafka

| Field | Type | Required | Description |
|-------|------|:--------:|-------------|
| `type` | string | ✓ | `"KAFKA"` |
| `bootstrap_servers` | string | ✓ | Broker addresses. Supports `{{config.*}}`, `{{secret.*}}`. |
| `topic` | string | ✓ | Target topic. Supports `{{config.*}}`. |
| `acks` | string | | `"0"`, `"1"`, `"all"`. Default: `"all"`. |
| `partition_key` | string | | Key for partitioning. Supports `{{input.*}}`, `{{config.*}}`. |
| `headers` | map | | Message headers. Supports `{{config.*}}`, `{{secret.*}}`. |
| `message_template` | object | ✓ | Message body. Supports `{{input.*}}`, `{{config.*}}`, `{{secret.*}}`. |
| `success_criteria` | object | | See below. |

##### `success_criteria` — Kafka

| Field | Type | Default | Description |
|-------|------|:-------:|-------------|
| `require_ack` | boolean | `true` | Wait for broker acknowledgement. |

#### `target` — Redis Streams

| Field | Type | Required | Description |
|-------|------|:--------:|-------------|
| `type` | string | ✓ | `"REDIS_STREAM"` |
| `uri` | string | ✓ | Redis connection URI. Supports `{{config.*}}`, `{{secret.*}}`. |
| `stream` | string | ✓ | Target stream name. Supports `{{config.*}}`. |
| `max_len` | integer | | `MAXLEN` for stream trimming. Optional. |
| `message_template` | object | ✓ | Message fields. Supports `{{input.*}}`, `{{config.*}}`, `{{secret.*}}`. |
| `success_criteria` | object | | See below. |

##### `success_criteria` — Redis Streams

| Field | Type | Default | Description |
|-------|------|:-------:|-------------|
| `require_ack` | boolean | `true` | Wait for stream append acknowledgement (message ID returned). |

#### `retry_policy` — Common Fields

| Field | Type | Default | Description |
|-------|------|:-------:|-------------|
| `max_attempts` | integer | `1` | Total attempts including first. `1` = no retries. |
| `backoff` | string | `exponential` | `fixed`, `linear`, `exponential`. |
| `initial_delay_ms` | integer | `1000` | Delay before first retry. |
| `max_delay_ms` | integer | `60000` | Upper bound on backoff. |

#### `retry_policy.retry_on` — HTTP

| Field | Type | Default | Description |
|-------|------|:-------:|-------------|
| `status_codes` | integer[] | `[500, 502, 503, 504]` | HTTP status codes that trigger retry. |
| `on_timeout` | boolean | `true` | Retry on request timeout. |

#### `retry_policy.retry_on` — Kafka

| Field | Type | Default | Description |
|-------|------|:-------:|-------------|
| `on_broker_error` | boolean | `true` | Retry on broker errors (leader not available, not enough replicas, etc.). |
| `on_timeout` | boolean | `true` | Retry on produce timeout. |

#### `retry_policy.retry_on` — Redis Streams

| Field | Type | Default | Description |
|-------|------|:-------:|-------------|
| `on_connection_error` | boolean | `true` | Retry on Redis connection failures. |
| `on_timeout` | boolean | `true` | Retry on command timeout. |

---

## Step 3 — Invoke

### `POST /jobs`

Invoke an endpoint. Specify when to fire and with what input.

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
  "endpoint": "generate-weekly-report",
  "trigger": "CRON",
  "cron": "0 9 * * MON",
  "timezone": "Asia/Kolkata",
  "starts_at": "2026-03-16T00:00:00Z",
  "ends_at": null,
  "input": {
    "report_type": "weekly"
  }
}
```

### Responses

**`201 Created` — Immediate / Delayed:**

```json
{
  "job_id": "job_8f3a...",
  "endpoint": "send-welcome-email",
  "trigger": "IMMEDIATE",
  "status": "ACTIVE",
  "version": 1,
  "idempotency_key": "order-1234-welcome-email",
  "input": { "user_id": "u_abc", "order_id": "order-1234" },
  "execution": {
    "execution_id": "exec_2b7c...",
    "status": "QUEUED",
    "created_at": "2026-03-15T10:00:00Z"
  },
  "created_at": "2026-03-15T10:00:00Z"
}
```

**`201 Created` — Cron:**

```json
{
  "job_id": "job_c72f...",
  "endpoint": "generate-weekly-report",
  "trigger": "CRON",
  "status": "ACTIVE",
  "version": 1,
  "cron": "0 9 * * MON",
  "timezone": "Asia/Kolkata",
  "starts_at": "2026-03-16T00:00:00Z",
  "ends_at": null,
  "next_run_at": "2026-03-16T09:00:00+05:30",
  "input": { "report_type": "weekly" },
  "created_at": "2026-03-15T10:00:00Z"
}
```

**`200 OK`** — Deduplicated, returns existing job.

### Job Creation — Field Reference

| Field | Type | Required | Description |
|-------|------|:--------:|-------------|
| `endpoint` | string | ✓ | Name of a registered endpoint. |
| `trigger` | string | ✓ | `IMMEDIATE`, `DELAYED`, `CRON`. |
| `idempotency_key` | string | ✓* | Deduplication key. *Required for `IMMEDIATE` and `DELAYED`. |
| `input` | object | | Execution payload. Validated against endpoint's payload spec. Static for `CRON` (used for every tick). |
| `run_at` | ISO 8601 | | Required for `DELAYED`. |
| `cron` | string | | Required for `CRON`. 5-field cron expression. |
| `timezone` | string | | Required for `CRON`. IANA timezone. |
| `starts_at` | ISO 8601 | | Optional for `CRON`. Default: now. |
| `ends_at` | ISO 8601 | | Optional for `CRON`. `null` = indefinite. |

---

## Job Lifecycle

### `GET /jobs/{job_id}`

Get full job details.

### `PUT /jobs/{job_id}` — Update (CRON only)

Creates a new version, retires the old one. Returns `409 JOB_NOT_UPDATABLE` for one-shot jobs.

**Request:**

```json
{
  "cron": "0 10 * * MON",
  "timezone": "Asia/Kolkata",
  "input": {
    "report_type": "weekly",
    "include_charts": true
  }
}
```

**Response — `201 Created`:**

```json
{
  "job_id": "job_e41b...",
  "endpoint": "generate-weekly-report",
  "trigger": "CRON",
  "status": "ACTIVE",
  "version": 2,
  "previous_version_id": "job_c72f...",
  "cron": "0 10 * * MON",
  "timezone": "Asia/Kolkata",
  "next_run_at": "2026-03-16T10:00:00+05:30",
  "input": { "report_type": "weekly", "include_charts": true },
  "created_at": "2026-03-15T11:00:00Z"
}
```

### `POST /jobs/{job_id}/cancel` — clearTimeout / clearInterval

**CRON jobs:** sets status to `RETIRED`, stops future executions. In-flight executions run to completion.

**One-shot jobs:** cancels execution if `PENDING` or `QUEUED`. Returns `409` if already `RUNNING`.

**Response — `200 OK`:**

```json
{
  "job_id": "job_c72f...",
  "status": "RETIRED",
  "retired_at": "2026-03-15T12:00:00Z"
}
```

### `GET /jobs/{job_id}/versions`

Full version chain for CRON jobs, newest to oldest.

### `GET /jobs/{job_id}/status`

Health overview for CRON jobs.

```json
{
  "job_id": "job_c72f...",
  "endpoint": "generate-weekly-report",
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

  "active_executions": {
    "pending": 2,
    "running": 1,
    "total": 3
  },

  "cron": {
    "expression": "0 9 * * MON",
    "next_run_at": "2026-03-16T09:00:00+05:30",
    "last_tick_at": "2026-03-09T09:00:00+05:30"
  },

  "stats": {
    "last_24h": {
      "total": 142,
      "succeeded": 139,
      "failed": 3,
      "avg_duration_ms": 340,
      "p99_duration_ms": 1200
    }
  }
}
```

| Health | Condition |
|--------|-----------|
| `HEALTHY` | Recent executions mostly succeeding. |
| `DEGRADED` | Elevated failure rate, but some succeeding. |
| `FAILING` | Most recent executions failing. |
| `IDLE` | No executions in recent window. |

### `GET /jobs`

| Param | Type | Default | Description |
|-------|------|:-------:|-------------|
| `endpoint` | string | | Filter by endpoint name. |
| `trigger` | string | | Filter: `IMMEDIATE`, `DELAYED`, `CRON`. |
| `status` | string | | Filter: `ACTIVE`, `RETIRED`. |
| `from` | ISO 8601 | | Start of time range. |
| `to` | ISO 8601 | | End of time range. |
| `limit` | integer | `50` | Page size. Max `200`. |
| `cursor` | string | | Pagination cursor. |

---

## Error Responses

```json
{
  "error": {
    "code": "JOB_NOT_FOUND",
    "message": "Job with ID 'job_xyz' does not exist.",
    "request_id": "req_9a8b..."
  }
}
```

| HTTP Status | Code | Description |
|:-----------:|------|-------------|
| `400` | `INVALID_REQUEST` | Malformed request body. |
| `401` | `UNAUTHORIZED` | Missing or invalid API key. |
| `404` | `PAYLOAD_SPEC_NOT_FOUND` | Payload spec name does not exist. |
| `404` | `CONFIG_NOT_FOUND` | Config name does not exist. |
| `404` | `SECRET_NOT_FOUND` | Secret name does not exist. |
| `404` | `ENDPOINT_NOT_FOUND` | Endpoint name does not exist. |
| `404` | `JOB_NOT_FOUND` | Job ID does not exist. |
| `404` | `EXECUTION_NOT_FOUND` | Execution ID does not exist. |
| `409` | `CONFLICT` | Cannot delete resource with active dependents. |
| `409` | `JOB_NOT_UPDATABLE` | Cannot update one-shot jobs. |
| `409` | `EXECUTION_NOT_CANCELLABLE` | Execution already running or completed. |
| `422` | `INVALID_CRON` | Invalid cron expression. |
| `422` | `INVALID_PAYLOAD_SPEC` | Payload spec JSON is not valid JSON Schema. |
| `422` | `INVALID_PAYLOAD_SPEC_REF` | Endpoint's payload spec reference not found. |
| `422` | `INVALID_CONFIG_REF` | Endpoint's config reference not found. |
| `422` | `INVALID_TARGET_TYPE` | Unknown or unsupported target type. |
| `422` | `INPUT_VALIDATION_FAILED` | Input does not match payload spec. |
| `422` | `TEMPLATE_RESOLUTION_FAILED` | Template variable unresolvable at runtime. |
| `429` | `RATE_LIMITED` | Too many requests. |
| `500` | `INTERNAL_ERROR` | Unexpected server error. |

---

## Rate Limits

| Endpoint | Limit |
|----------|-------|
| `POST /jobs` | 1000 req/s per endpoint |
| `GET /jobs/{job_id}/status` | 100 req/s per job |
| All other endpoints | 200 req/s per API key |

```
X-RateLimit-Limit: 1000
X-RateLimit-Remaining: 997
X-RateLimit-Reset: 1742108400
```

---

## Endpoint Summary

### Step 1 — Setup

| Method | Path | Description |
|--------|------|-------------|
| `POST` | `/payload-specs` | Create a payload spec |
| `GET` | `/payload-specs` | List payload specs |
| `GET` | `/payload-specs/{name}` | Get a payload spec |
| `PUT` | `/payload-specs/{name}` | Update a payload spec |
| `DELETE` | `/payload-specs/{name}` | Delete a payload spec |
| `POST` | `/configs` | Create a config |
| `GET` | `/configs` | List configs |
| `GET` | `/configs/{name}` | Get a config |
| `PUT` | `/configs/{name}` | Update a config |
| `DELETE` | `/configs/{name}` | Delete a config |
| `POST` | `/secrets` | Create a secret |
| `GET` | `/secrets` | List secrets (names only) |
| `GET` | `/secrets/{name}` | Get secret metadata |
| `PUT` | `/secrets/{name}` | Rotate a secret |
| `DELETE` | `/secrets/{name}` | Delete a secret |

### Step 2 — Register

| Method | Path | Description |
|--------|------|-------------|
| `POST` | `/endpoints` | Register an endpoint |
| `GET` | `/endpoints` | List endpoints |
| `GET` | `/endpoints/{name}` | Get an endpoint |
| `PUT` | `/endpoints/{name}` | Update an endpoint |
| `DELETE` | `/endpoints/{name}` | Delete an endpoint |

### Step 3 — Invoke

| Method | Path | Description |
|--------|------|-------------|
| `POST` | `/jobs` | Create a job (triggers execution) |
| `GET` | `/jobs` | List jobs |
| `GET` | `/jobs/{job_id}` | Get a job |
| `PUT` | `/jobs/{job_id}` | Update CRON job (new version) |
| `POST` | `/jobs/{job_id}/cancel` | Cancel / retire a job |
| `GET` | `/jobs/{job_id}/status` | Job health and stats |
| `GET` | `/jobs/{job_id}/versions` | Version history |
