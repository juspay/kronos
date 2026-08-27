-- Workspace-scoped tables.
--
-- Two placeholders are substituted at runtime by `db::workspaces::render_workspace_ddl`:
--
--   {s}  the quoted workspace schema, e.g. "acme_prod". Every table reference is
--        qualified with it so provisioning needs no `SET search_path` — a session
--        SET pins the backend connection for the life of the client session behind
--        a transaction-pooling proxy (RDS Proxy, PgBouncer), destroying multiplexing.
--        Constraint and index *names* are deliberately left unqualified: they are
--        local to the table's schema and Postgres rejects a qualified name there.
--
--   {p}  the configured table prefix, used as-is including any trailing separator
--        (e.g. "sched_"), or empty string for no prefix.

-- Pinned to `public` rather than left to search_path resolution, so the extension
-- lands in one predictable place instead of inside whichever workspace schema
-- happened to be provisioned first. Note `gen_random_uuid()` below resolves from
-- pg_catalog on PostgreSQL 13+ regardless, since pg_catalog is always implicitly
-- first in the resolution order.
CREATE EXTENSION IF NOT EXISTS pgcrypto WITH SCHEMA public;

CREATE TABLE IF NOT EXISTS {s}.{p}payload_specs (
    name          TEXT        NOT NULL,
    schema_json   JSONB       NOT NULL,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT pk_{p}payload_specs PRIMARY KEY (name)
);

CREATE TABLE IF NOT EXISTS {s}.{p}configs (
    name          TEXT        NOT NULL,
    values_json   JSONB       NOT NULL,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT pk_{p}configs PRIMARY KEY (name)
);

CREATE TABLE IF NOT EXISTS {s}.{p}secrets (
    name              TEXT        NOT NULL,
    encrypted_value   BYTEA       NOT NULL,
    created_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT pk_{p}secrets PRIMARY KEY (name)
);

CREATE TABLE IF NOT EXISTS {s}.{p}endpoints (
    name              TEXT        NOT NULL,
    endpoint_type     TEXT        NOT NULL,
    payload_spec_ref  TEXT,
    config_ref        TEXT,
    spec              JSONB       NOT NULL,
    retry_policy      JSONB,
    created_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT pk_{p}endpoints PRIMARY KEY (name),
    CONSTRAINT fk_{p}endpoints_payload_spec FOREIGN KEY (payload_spec_ref) REFERENCES {s}.{p}payload_specs (name),
    CONSTRAINT fk_{p}endpoints_config FOREIGN KEY (config_ref) REFERENCES {s}.{p}configs (name),
    CONSTRAINT chk_{p}endpoint_type CHECK (endpoint_type IN ('HTTP', 'KAFKA', 'REDIS_STREAM', 'INTERNAL'))
);

CREATE INDEX IF NOT EXISTS idx_{p}endpoints_type ON {s}.{p}endpoints (endpoint_type);

CREATE TABLE IF NOT EXISTS {s}.{p}jobs (
    job_id                TEXT        NOT NULL DEFAULT gen_random_uuid()::TEXT,
    endpoint              TEXT        NOT NULL,
    endpoint_type         TEXT        NOT NULL,
    trigger_type          TEXT        NOT NULL,
    status                TEXT        NOT NULL DEFAULT 'ACTIVE',
    version               BIGINT      NOT NULL DEFAULT 1,
    previous_version_id   TEXT,
    replaced_by_id        TEXT,
    idempotency_key       TEXT,
    input                 JSONB,
    run_at                TIMESTAMPTZ,
    cron_expression       TEXT,
    cron_timezone         TEXT,
    cron_starts_at        TIMESTAMPTZ,
    cron_ends_at          TIMESTAMPTZ,
    cron_next_run_at      TIMESTAMPTZ,
    cron_last_tick_at     TIMESTAMPTZ,
    created_at            TIMESTAMPTZ NOT NULL DEFAULT now(),
    retired_at            TIMESTAMPTZ,
    CONSTRAINT pk_{p}jobs PRIMARY KEY (job_id),
    CONSTRAINT fk_{p}jobs_endpoint FOREIGN KEY (endpoint) REFERENCES {s}.{p}endpoints (name),
    CONSTRAINT chk_{p}trigger_type CHECK (trigger_type IN ('IMMEDIATE', 'DELAYED', 'CRON')),
    CONSTRAINT chk_{p}job_status CHECK (status IN ('ACTIVE', 'RETIRED')),
    CONSTRAINT chk_{p}job_endpoint_type CHECK (endpoint_type IN ('HTTP', 'KAFKA', 'REDIS_STREAM', 'INTERNAL'))
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_{p}jobs_idempotency
    ON {s}.{p}jobs (endpoint, idempotency_key)
    WHERE idempotency_key IS NOT NULL;

CREATE INDEX IF NOT EXISTS idx_{p}jobs_cron_due
    ON {s}.{p}jobs (cron_next_run_at)
    WHERE trigger_type = 'CRON' AND status = 'ACTIVE';

CREATE INDEX IF NOT EXISTS idx_{p}jobs_endpoint ON {s}.{p}jobs (endpoint, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_{p}jobs_status   ON {s}.{p}jobs (status,   created_at DESC);

CREATE TABLE IF NOT EXISTS {s}.{p}executions (
    execution_id    TEXT        NOT NULL DEFAULT gen_random_uuid()::TEXT,
    job_id          TEXT        NOT NULL,
    endpoint        TEXT        NOT NULL,
    endpoint_type   TEXT        NOT NULL,
    idempotency_key TEXT,
    status          TEXT        NOT NULL DEFAULT 'PENDING',
    input           JSONB,
    output          JSONB,
    attempt_count   BIGINT      NOT NULL DEFAULT 0,
    max_attempts    BIGINT      NOT NULL DEFAULT 1,
    worker_id       TEXT,
    run_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    started_at      TIMESTAMPTZ,
    completed_at    TIMESTAMPTZ,
    duration_ms     BIGINT,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT pk_{p}executions PRIMARY KEY (execution_id),
    CONSTRAINT fk_{p}executions_job FOREIGN KEY (job_id) REFERENCES {s}.{p}jobs (job_id),
    CONSTRAINT chk_{p}exec_status CHECK (status IN (
        'PENDING', 'QUEUED', 'RUNNING', 'RETRYING', 'SUCCESS', 'FAILED', 'CANCELLED'
    ))
);

CREATE INDEX IF NOT EXISTS idx_{p}executions_pickup
    ON {s}.{p}executions (status, run_at ASC)
    WHERE status IN ('QUEUED', 'RETRYING', 'PENDING');

CREATE UNIQUE INDEX IF NOT EXISTS idx_{p}executions_cron_dedup
    ON {s}.{p}executions (job_id, idempotency_key)
    WHERE idempotency_key IS NOT NULL;

CREATE INDEX IF NOT EXISTS idx_{p}executions_by_job  ON {s}.{p}executions (job_id,  created_at DESC);
CREATE INDEX IF NOT EXISTS idx_{p}executions_running ON {s}.{p}executions (status,   started_at)
    WHERE status = 'RUNNING';

CREATE TABLE IF NOT EXISTS {s}.{p}attempts (
    attempt_id      TEXT        NOT NULL DEFAULT gen_random_uuid()::TEXT,
    execution_id    TEXT        NOT NULL,
    attempt_number  BIGINT      NOT NULL,
    status          TEXT        NOT NULL,
    started_at      TIMESTAMPTZ NOT NULL,
    completed_at    TIMESTAMPTZ,
    duration_ms     BIGINT,
    output          JSONB,
    error           JSONB,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT pk_{p}attempts PRIMARY KEY (attempt_id),
    CONSTRAINT fk_{p}attempts_execution FOREIGN KEY (execution_id) REFERENCES {s}.{p}executions (execution_id),
    CONSTRAINT uq_{p}attempts_exec_number UNIQUE (execution_id, attempt_number),
    CONSTRAINT chk_{p}attempt_status CHECK (status IN ('SUCCESS', 'FAILED'))
);

CREATE INDEX IF NOT EXISTS idx_{p}attempts_by_execution
    ON {s}.{p}attempts (execution_id, attempt_number ASC);

CREATE TABLE IF NOT EXISTS {s}.{p}execution_logs (
    log_id          TEXT        NOT NULL DEFAULT gen_random_uuid()::TEXT,
    execution_id    TEXT        NOT NULL,
    attempt_number  BIGINT      NOT NULL,
    level           TEXT        NOT NULL,
    message         TEXT        NOT NULL,
    logged_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT pk_{p}execution_logs PRIMARY KEY (log_id),
    CONSTRAINT fk_{p}logs_execution FOREIGN KEY (execution_id) REFERENCES {s}.{p}executions (execution_id),
    CONSTRAINT chk_{p}log_level CHECK (level IN ('DEBUG', 'INFO', 'WARN', 'ERROR'))
);

CREATE INDEX IF NOT EXISTS idx_{p}logs_by_execution
    ON {s}.{p}execution_logs (execution_id, logged_at ASC);
CREATE INDEX IF NOT EXISTS idx_{p}logs_by_attempt
    ON {s}.{p}execution_logs (execution_id, attempt_number, logged_at ASC);
