# Task Executor — CockroachDB Schema Design

**Version:** 2.0.0
**Date:** March 15, 2026

---

## Overview

This document defines the CockroachDB schema for the Task Executor platform running in an active-active configuration across `ap-south-1` and `ap-south-2`. The platform supports three delivery transports: HTTP, Kafka, and Redis Streams.

**Design principles:** definitions are sacred (cross-region durable), work items are fast (local quorum, clients can retry).

---

## Database Setup

```sql
CREATE DATABASE taskexecutor;
USE taskexecutor;

ALTER DATABASE taskexecutor SET PRIMARY REGION = 'ap-south-1';
ALTER DATABASE taskexecutor ADD REGION 'ap-south-2';
ALTER DATABASE taskexecutor SURVIVE ZONE FAILURE;
```

---

## Table Locality Strategy

| Table | Locality | Write frequency | Rationale |
|-------|----------|----------------|-----------|
| `payload_specs` | `GLOBAL` | Rare | Read-heavy definitions. Must survive region failure. |
| `configs` | `GLOBAL` | Rare | Same as above. |
| `secrets` | `GLOBAL` | Rare | Same as above. Encrypted at rest. |
| `endpoints` | `GLOBAL` | Rare | Registered once, read on every execution. |
| `jobs` | `REGIONAL BY ROW` | High | Created per invocation. Workers only access local region. |
| `executions` | `REGIONAL BY ROW` | High | One per job fire. Hot path must be local. |
| `attempts` | `REGIONAL BY ROW` | High | One per retry. Co-located with parent execution. |
| `execution_logs` | `REGIONAL BY ROW` | Very high | Append-only. Local to execution. |
| `region_heartbeats` | `GLOBAL` | Moderate | Must be readable from any region for health checking. |
| `region_status` | `GLOBAL` | Rare | Failover coordination. Global ensures split-brain protection. |

---

## Schema Definitions

### Payload Specs

```sql
CREATE TABLE payload_specs (
    name          STRING        NOT NULL,
    schema_json   JSONB         NOT NULL,      -- JSON Schema definition
    created_at    TIMESTAMPTZ   NOT NULL DEFAULT now(),
    updated_at    TIMESTAMPTZ   NOT NULL DEFAULT now(),

    CONSTRAINT pk_payload_specs PRIMARY KEY (name)
);

ALTER TABLE payload_specs SET LOCALITY GLOBAL;
```

---

### Configs

```sql
CREATE TABLE configs (
    name          STRING        NOT NULL,
    values_json   JSONB         NOT NULL,
    created_at    TIMESTAMPTZ   NOT NULL DEFAULT now(),
    updated_at    TIMESTAMPTZ   NOT NULL DEFAULT now(),

    CONSTRAINT pk_configs PRIMARY KEY (name)
);

ALTER TABLE configs SET LOCALITY GLOBAL;
```

---

### Secrets

```sql
CREATE TABLE secrets (
    name              STRING        NOT NULL,
    encrypted_value   BYTES         NOT NULL,      -- encrypted at rest, never returned via API
    created_at        TIMESTAMPTZ   NOT NULL DEFAULT now(),
    updated_at        TIMESTAMPTZ   NOT NULL DEFAULT now(),

    CONSTRAINT pk_secrets PRIMARY KEY (name)
);

ALTER TABLE secrets SET LOCALITY GLOBAL;
```

---

### Endpoints

```sql
CREATE TABLE endpoints (
    name              STRING        NOT NULL,
    endpoint_type     STRING        NOT NULL,      -- HTTP, KAFKA, REDIS_STREAM
    payload_spec_ref  STRING,                      -- references payload_specs.name
    config_ref        STRING,                      -- references configs.name
    spec              JSONB         NOT NULL,       -- HttpSpec | KafkaSpec | RedisStreamSpec
    retry_policy      JSONB,                        -- RetryPolicy
    created_at        TIMESTAMPTZ   NOT NULL DEFAULT now(),
    updated_at        TIMESTAMPTZ   NOT NULL DEFAULT now(),

    CONSTRAINT pk_endpoints PRIMARY KEY (name),
    CONSTRAINT fk_endpoints_payload_spec FOREIGN KEY (payload_spec_ref) 
        REFERENCES payload_specs (name),
    CONSTRAINT fk_endpoints_config FOREIGN KEY (config_ref) REFERENCES configs (name),
    CONSTRAINT chk_endpoint_type CHECK (endpoint_type IN ('HTTP', 'KAFKA', 'REDIS_STREAM'))
);

ALTER TABLE endpoints SET LOCALITY GLOBAL;

-- Filter endpoints by type
CREATE INDEX idx_endpoints_type ON endpoints (endpoint_type);
```

**`spec` JSONB — shape depends on `endpoint_type`:**

HTTP:
```json
{
  "url": "{{config.api_base_url}}/emails/welcome",
  "method": "POST",
  "headers": { "Authorization": "Bearer {{secret.email_api_key}}" },
  "body_template": { "user_id": "{{input.user_id}}" },
  "timeout_ms": 5000,
  "expected_status_codes": [200, 201, 202, 204]
}
```

Kafka:
```json
{
  "bootstrap_servers": "{{config.bootstrap_servers}}",
  "topic": "{{config.topic}}",
  "key_template": "{{input.order_id}}",
  "value_template": { "event_type": "{{input.event_type}}", "order_id": "{{input.order_id}}" },
  "headers": { "ce-type": "order.{{input.event_type}}" },
  "acks": "all",
  "timeout_ms": 10000
}
```

Redis Stream:
```json
{
  "redis_url": "{{config.redis_url}}",
  "stream": "{{config.stream_name}}",
  "fields_template": { "user_id": "{{input.user_id}}", "title": "{{input.title}}" },
  "max_len": 100000,
  "approximate_trimming": true,
  "timeout_ms": 3000
}
```

---

### Jobs

```sql
CREATE TABLE jobs (
    job_id                STRING        NOT NULL DEFAULT gen_random_uuid()::STRING,
    crdb_region           crdb_internal_region NOT NULL DEFAULT gateway_region()::crdb_internal_region,
    endpoint              STRING        NOT NULL,
    endpoint_type         STRING        NOT NULL,      -- denormalized from endpoints for worker routing
    trigger_type          STRING        NOT NULL,      -- IMMEDIATE, DELAYED, CRON
    status                STRING        NOT NULL DEFAULT 'ACTIVE',
    version               INT           NOT NULL DEFAULT 1,
    previous_version_id   STRING,
    replaced_by_id        STRING,
    idempotency_key       STRING,
    input                 JSONB,
    run_at                TIMESTAMPTZ,                 -- DELAYED only
    cron_expression       STRING,                      -- CRON only
    cron_timezone         STRING,                      -- CRON only
    cron_starts_at        TIMESTAMPTZ,                 -- CRON only
    cron_ends_at          TIMESTAMPTZ,                 -- CRON only
    cron_next_run_at      TIMESTAMPTZ,                 -- CRON only, computed
    cron_last_tick_at     TIMESTAMPTZ,                 -- CRON only, updated by scheduler
    created_at            TIMESTAMPTZ   NOT NULL DEFAULT now(),
    retired_at            TIMESTAMPTZ,

    CONSTRAINT pk_jobs PRIMARY KEY (crdb_region, job_id),
    CONSTRAINT fk_jobs_endpoint FOREIGN KEY (endpoint) REFERENCES endpoints (name),
    CONSTRAINT chk_trigger_type CHECK (trigger_type IN ('IMMEDIATE', 'DELAYED', 'CRON')),
    CONSTRAINT chk_status CHECK (status IN ('ACTIVE', 'RETIRED')),
    CONSTRAINT chk_endpoint_type CHECK (endpoint_type IN ('HTTP', 'KAFKA', 'REDIS_STREAM'))
);

ALTER TABLE jobs SET LOCALITY REGIONAL BY ROW;

-- Deduplication
CREATE UNIQUE INDEX idx_jobs_idempotency
    ON jobs (endpoint, idempotency_key)
    WHERE idempotency_key IS NOT NULL;

-- Scheduler: CRON jobs due for next tick
CREATE INDEX idx_jobs_cron_due
    ON jobs (crdb_region, cron_next_run_at)
    WHERE trigger_type = 'CRON' AND status = 'ACTIVE';

-- List by endpoint
CREATE INDEX idx_jobs_endpoint
    ON jobs (crdb_region, endpoint, created_at DESC);

-- List by status
CREATE INDEX idx_jobs_status
    ON jobs (crdb_region, status, created_at DESC);

-- List by endpoint type (useful for transport-specific worker pools)
CREATE INDEX idx_jobs_endpoint_type
    ON jobs (crdb_region, endpoint_type, created_at DESC);
```

---

### Executions

```sql
CREATE TABLE executions (
    execution_id    STRING        NOT NULL DEFAULT gen_random_uuid()::STRING,
    crdb_region     crdb_internal_region NOT NULL DEFAULT gateway_region()::crdb_internal_region,
    job_id          STRING        NOT NULL,
    endpoint        STRING        NOT NULL,
    endpoint_type   STRING        NOT NULL,            -- denormalized for worker routing
    idempotency_key STRING,
    status          STRING        NOT NULL DEFAULT 'PENDING',
    input           JSONB,
    output          JSONB,                             -- transport-specific success payload
    attempt_count   INT           NOT NULL DEFAULT 0,
    worker_id       STRING,
    run_at          TIMESTAMPTZ   NOT NULL DEFAULT now(),
    started_at      TIMESTAMPTZ,
    completed_at    TIMESTAMPTZ,
    duration_ms     INT,
    created_at      TIMESTAMPTZ   NOT NULL DEFAULT now(),

    CONSTRAINT pk_executions PRIMARY KEY (crdb_region, execution_id),
    CONSTRAINT fk_executions_job FOREIGN KEY (crdb_region, job_id) 
        REFERENCES jobs (crdb_region, job_id),
    CONSTRAINT chk_exec_status CHECK (status IN (
        'PENDING', 'QUEUED', 'RUNNING', 'RETRYING', 'SUCCESS', 'FAILED', 'CANCELLED'
    )),
    CONSTRAINT chk_exec_endpoint_type CHECK (endpoint_type IN ('HTTP', 'KAFKA', 'REDIS_STREAM'))
);

ALTER TABLE executions SET LOCALITY REGIONAL BY ROW;

-- Worker pickup: THE hot-path query
-- Workers can optionally filter by endpoint_type for transport-specific pools
CREATE INDEX idx_executions_pickup
    ON executions (crdb_region, status, run_at ASC)
    WHERE status IN ('QUEUED', 'RETRYING');

-- Transport-specific worker pickup
CREATE INDEX idx_executions_pickup_by_type
    ON executions (crdb_region, endpoint_type, status, run_at ASC)
    WHERE status IN ('QUEUED', 'RETRYING');

-- CRON tick deduplication
CREATE UNIQUE INDEX idx_executions_cron_dedup
    ON executions (crdb_region, job_id, idempotency_key)
    WHERE idempotency_key IS NOT NULL;

-- List by job
CREATE INDEX idx_executions_by_job
    ON executions (crdb_region, job_id, created_at DESC);

-- Find stuck executions
CREATE INDEX idx_executions_running
    ON executions (crdb_region, status, started_at)
    WHERE status = 'RUNNING';
```

**`output` JSONB — shape depends on transport:**

HTTP:
```json
{ "status_code": 200, "body": "OK", "response_headers": { ... } }
```

Kafka:
```json
{ "partition": 3, "offset": 12847, "timestamp": "2026-03-15T10:30:01Z" }
```

Redis Stream:
```json
{ "message_id": "1710499801234-0", "stream": "notifications:outbound" }
```

---

### Attempts

```sql
CREATE TABLE attempts (
    attempt_id      STRING        NOT NULL DEFAULT gen_random_uuid()::STRING,
    crdb_region     crdb_internal_region NOT NULL DEFAULT gateway_region()::crdb_internal_region,
    execution_id    STRING        NOT NULL,
    attempt_number  INT           NOT NULL,
    status          STRING        NOT NULL,
    started_at      TIMESTAMPTZ   NOT NULL,
    completed_at    TIMESTAMPTZ,
    duration_ms     INT,
    output          JSONB,                             -- transport-specific on success
    error           JSONB,                             -- { type, message } on failure
    created_at      TIMESTAMPTZ   NOT NULL DEFAULT now(),

    CONSTRAINT pk_attempts PRIMARY KEY (crdb_region, attempt_id),
    CONSTRAINT fk_attempts_execution FOREIGN KEY (crdb_region, execution_id)
        REFERENCES executions (crdb_region, execution_id),
    CONSTRAINT uq_attempts_exec_number UNIQUE (crdb_region, execution_id, attempt_number),
    CONSTRAINT chk_attempt_status CHECK (status IN ('SUCCESS', 'FAILED'))
);

ALTER TABLE attempts SET LOCALITY REGIONAL BY ROW;

CREATE INDEX idx_attempts_by_execution
    ON attempts (crdb_region, execution_id, attempt_number ASC);
```

**`error` JSONB — type varies by transport:**

HTTP:
```json
{ "type": "TIMEOUT", "message": "Request timed out after 5000ms" }
{ "type": "HTTP_ERROR", "message": "Received status 503", "status_code": 503 }
{ "type": "CONNECTION_ERROR", "message": "Connection refused" }
```

Kafka:
```json
{ "type": "BROKER_ERROR", "message": "Leader not available for partition 3" }
{ "type": "TIMEOUT", "message": "Produce timed out after 10000ms" }
{ "type": "SERIALIZATION_ERROR", "message": "Failed to serialize message key" }
```

Redis Stream:
```json
{ "type": "CONNECTION_ERROR", "message": "Redis connection refused" }
{ "type": "TIMEOUT", "message": "XADD timed out after 3000ms" }
{ "type": "STREAM_ERROR", "message": "WRONGTYPE Operation against a key holding the wrong kind of value" }
```

---

### Execution Logs

```sql
CREATE TABLE execution_logs (
    log_id          STRING        NOT NULL DEFAULT gen_random_uuid()::STRING,
    crdb_region     crdb_internal_region NOT NULL DEFAULT gateway_region()::crdb_internal_region,
    execution_id    STRING        NOT NULL,
    attempt_number  INT           NOT NULL,
    level           STRING        NOT NULL,
    message         STRING        NOT NULL,
    logged_at       TIMESTAMPTZ   NOT NULL DEFAULT now(),

    CONSTRAINT pk_execution_logs PRIMARY KEY (crdb_region, log_id),
    CONSTRAINT fk_logs_execution FOREIGN KEY (crdb_region, execution_id)
        REFERENCES executions (crdb_region, execution_id),
    CONSTRAINT chk_log_level CHECK (level IN ('DEBUG', 'INFO', 'WARN', 'ERROR'))
);

ALTER TABLE execution_logs SET LOCALITY REGIONAL BY ROW;

CREATE INDEX idx_logs_by_execution
    ON execution_logs (crdb_region, execution_id, logged_at ASC);

CREATE INDEX idx_logs_by_attempt
    ON execution_logs (crdb_region, execution_id, attempt_number, logged_at ASC);
```

---

### Region Heartbeats

```sql
CREATE TABLE region_heartbeats (
    region        STRING        NOT NULL,
    component     STRING        NOT NULL,
    last_beat_at  TIMESTAMPTZ   NOT NULL DEFAULT now(),
    status        STRING        NOT NULL DEFAULT 'ALIVE',
    metadata      JSONB,

    CONSTRAINT pk_region_heartbeats PRIMARY KEY (region, component),
    CONSTRAINT chk_hb_status CHECK (status IN ('ALIVE', 'DEGRADED', 'DEAD'))
);

ALTER TABLE region_heartbeats SET LOCALITY GLOBAL;
```

---

### Region Status

```sql
CREATE TABLE region_status (
    region        STRING        NOT NULL,
    alive         BOOL          NOT NULL DEFAULT true,
    failed_at     TIMESTAMPTZ,
    adopted_by    STRING,
    updated_at    TIMESTAMPTZ   NOT NULL DEFAULT now(),

    CONSTRAINT pk_region_status PRIMARY KEY (region)
);

ALTER TABLE region_status SET LOCALITY GLOBAL;

INSERT INTO region_status (region, alive, updated_at) VALUES
    ('ap-south-1', true, now()),
    ('ap-south-2', true, now());
```

---

## Key Queries

### Worker Pickup — All Transports (Generic Pool)

```sql
UPDATE executions
SET status = 'RUNNING',
    worker_id = $1,
    started_at = now(),
    attempt_count = attempt_count + 1
WHERE (crdb_region, execution_id) = (
    SELECT crdb_region, execution_id
    FROM executions
    WHERE crdb_region = 'ap-south-1'
      AND status IN ('QUEUED', 'RETRYING')
      AND run_at <= now()
    ORDER BY run_at ASC
    LIMIT 1
    FOR UPDATE SKIP LOCKED
)
RETURNING *;
```

### Worker Pickup — Transport-Specific Pool

If you run separate worker pools per transport (e.g., HTTP workers, Kafka workers):

```sql
UPDATE executions
SET status = 'RUNNING',
    worker_id = $1,
    started_at = now(),
    attempt_count = attempt_count + 1
WHERE (crdb_region, execution_id) = (
    SELECT crdb_region, execution_id
    FROM executions
    WHERE crdb_region = 'ap-south-1'
      AND endpoint_type = 'KAFKA'          -- this worker only handles Kafka
      AND status IN ('QUEUED', 'RETRYING')
      AND run_at <= now()
    ORDER BY run_at ASC
    LIMIT 1
    FOR UPDATE SKIP LOCKED
)
RETURNING *;
```

### Worker Pickup — Failover Mode

```sql
UPDATE executions
SET status = 'RUNNING',
    worker_id = $1,
    started_at = now(),
    attempt_count = attempt_count + 1
WHERE (crdb_region, execution_id) = (
    SELECT e.crdb_region, e.execution_id
    FROM executions e
    WHERE e.crdb_region IN (
        'ap-south-2',
        (SELECT region FROM region_status
         WHERE region = 'ap-south-1' AND alive = false)
    )
      AND e.status IN ('QUEUED', 'RETRYING')
      AND e.run_at <= now()
    ORDER BY e.run_at ASC
    LIMIT 1
    FOR UPDATE SKIP LOCKED
)
RETURNING *;
```

### Job Creation — Immediate

```sql
BEGIN;

INSERT INTO jobs (endpoint, endpoint_type, trigger_type, idempotency_key, input)
VALUES ($1, $2, 'IMMEDIATE', $3, $4)
RETURNING job_id, crdb_region;

INSERT INTO executions (crdb_region, job_id, endpoint, endpoint_type, idempotency_key, status, run_at, input)
VALUES ($5, $6, $1, $2, $3, 'QUEUED', now(), $4)
RETURNING execution_id, status, created_at;

COMMIT;
```

### Job Creation — Delayed

```sql
BEGIN;

INSERT INTO jobs (endpoint, endpoint_type, trigger_type, idempotency_key, input, run_at)
VALUES ($1, $2, 'DELAYED', $3, $4, $5)
RETURNING job_id, crdb_region;

INSERT INTO executions (crdb_region, job_id, endpoint, endpoint_type, idempotency_key, status, run_at, input)
VALUES ($6, $7, $1, $2, $3, 'PENDING', $5, $4)
RETURNING execution_id, status, created_at;

COMMIT;
```

### CRON Tick Materialization

```sql
-- Step 1: Find due CRON jobs
SELECT job_id, endpoint, endpoint_type, input, cron_expression, cron_next_run_at
FROM jobs
WHERE crdb_region = 'ap-south-1'
  AND trigger_type = 'CRON'
  AND status = 'ACTIVE'
  AND cron_next_run_at <= now()
  AND (cron_ends_at IS NULL OR cron_ends_at > now());

-- Step 2: Create execution with dedup key
INSERT INTO executions (crdb_region, job_id, endpoint, endpoint_type, idempotency_key, status, input, run_at)
VALUES (
    'ap-south-1', $1, $2, $3,
    'cron_' || $1 || '_' || extract(epoch from $4)::TEXT,
    'QUEUED', $5, $4
)
ON CONFLICT (crdb_region, job_id, idempotency_key) DO NOTHING;

-- Step 3: Advance next_run_at
UPDATE jobs
SET cron_next_run_at = $6, cron_last_tick_at = $4
WHERE crdb_region = 'ap-south-1' AND job_id = $1;
```

### Delayed Job Promotion

```sql
UPDATE executions
SET status = 'QUEUED'
WHERE crdb_region = 'ap-south-1'
  AND status = 'PENDING'
  AND run_at <= now();
```

### Execution Completion

```sql
UPDATE executions
SET status = 'SUCCESS',
    output = $2,
    completed_at = now(),
    duration_ms = extract(epoch from (now() - started_at))::INT * 1000
WHERE crdb_region = $3
  AND execution_id = $1
  AND status = 'RUNNING';
```

### Execution Retry

```sql
UPDATE executions
SET status = CASE
        WHEN attempt_count >= $2 THEN 'FAILED'
        ELSE 'RETRYING'
    END,
    run_at = CASE
        WHEN attempt_count >= $2 THEN run_at
        ELSE now() + ($3 * interval '1 millisecond')
    END,
    worker_id = NULL
WHERE crdb_region = $4
  AND execution_id = $1
  AND status = 'RUNNING';
```

### Stuck Execution Recovery

```sql
UPDATE executions
SET status = 'RETRYING',
    worker_id = NULL,
    run_at = now()
WHERE crdb_region = 'ap-south-1'
  AND status = 'RUNNING'
  AND started_at < now() - interval '5 minutes';
```

---

## Worker Pool Architecture

Workers can be deployed in two ways:

**Generic pool** — every worker handles all transports. Simpler to operate. Each worker needs HTTP client, Kafka producer, and Redis client libraries.

**Transport-specific pools** — separate worker pools for HTTP, Kafka, and Redis Stream. More complex to operate but allows independent scaling. Kafka pool can be larger during high event volumes without affecting HTTP capacity.

```
Generic Pool:
  Worker → picks up any QUEUED execution → checks endpoint_type → dispatches

Transport-Specific Pools:
  HTTP Worker Pool    → WHERE endpoint_type = 'HTTP'
  Kafka Worker Pool   → WHERE endpoint_type = 'KAFKA'
  Redis Worker Pool   → WHERE endpoint_type = 'REDIS_STREAM'
```

The `idx_executions_pickup_by_type` index supports the transport-specific pattern without overhead for the generic pattern.

---

## Data Retention

```sql
ALTER TABLE executions SET (
    ttl_expiration_expression =
        CASE WHEN status IN ('SUCCESS', 'FAILED', 'CANCELLED')
             THEN created_at + INTERVAL '30 days'
             ELSE NULL
        END
);

ALTER TABLE attempts SET (
    ttl_expiration_expression = created_at + INTERVAL '30 days'
);

ALTER TABLE execution_logs SET (
    ttl_expiration_expression = logged_at + INTERVAL '14 days'
);

ALTER TABLE jobs SET (
    ttl_expiration_expression =
        CASE WHEN status = 'RETIRED'
             THEN retired_at + INTERVAL '90 days'
             ELSE NULL
        END
);
```

---

## Entity Relationship Summary

```
payload_specs ────────┐
                      │ referenced by
configs ──────────────┤
                      ▼
                  endpoints
                      │
                      │ referenced by
                      ▼
                    jobs
                      │
                      │ one-to-many (IMMEDIATE/DELAYED: 1, CRON: many)
                      ▼
                  executions
                      │
                      │ one-to-many
                      ▼
                   attempts
                      │
                      │ one-to-many
                      ▼
                execution_logs
```

---

## Index Summary

| Table | Index | Purpose |
|-------|-------|---------|
| `endpoints` | `idx_endpoints_type` | Filter endpoints by transport type |
| `jobs` | `idx_jobs_idempotency` | Deduplication on `(endpoint, idempotency_key)` |
| `jobs` | `idx_jobs_cron_due` | Scheduler finds due CRON jobs |
| `jobs` | `idx_jobs_endpoint` | List jobs by endpoint |
| `jobs` | `idx_jobs_status` | List jobs by status |
| `jobs` | `idx_jobs_endpoint_type` | List/filter by transport type |
| `executions` | `idx_executions_pickup` | **Hot path** — generic worker pickup |
| `executions` | `idx_executions_pickup_by_type` | **Hot path** — transport-specific worker pickup |
| `executions` | `idx_executions_cron_dedup` | Prevent duplicate CRON ticks |
| `executions` | `idx_executions_by_job` | List executions for a job |
| `executions` | `idx_executions_running` | Find stuck executions |
| `attempts` | `idx_attempts_by_execution` | List attempts for an execution |
| `execution_logs` | `idx_logs_by_execution` | Query logs for an execution |
| `execution_logs` | `idx_logs_by_attempt` | Query logs by attempt number |
