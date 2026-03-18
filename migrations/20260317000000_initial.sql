-- Task Executor initial schema

CREATE EXTENSION IF NOT EXISTS pgcrypto;

CREATE TABLE IF NOT EXISTS payload_specs (
    name          TEXT          NOT NULL,
    schema_json   JSONB         NOT NULL,
    created_at    TIMESTAMPTZ   NOT NULL DEFAULT now(),
    updated_at    TIMESTAMPTZ   NOT NULL DEFAULT now(),
    CONSTRAINT pk_payload_specs PRIMARY KEY (name)
);

CREATE TABLE IF NOT EXISTS configs (
    name          TEXT          NOT NULL,
    values_json   JSONB         NOT NULL,
    created_at    TIMESTAMPTZ   NOT NULL DEFAULT now(),
    updated_at    TIMESTAMPTZ   NOT NULL DEFAULT now(),
    CONSTRAINT pk_configs PRIMARY KEY (name)
);

CREATE TABLE IF NOT EXISTS secrets (
    name              TEXT          NOT NULL,
    encrypted_value   BYTEA         NOT NULL,
    created_at        TIMESTAMPTZ   NOT NULL DEFAULT now(),
    updated_at        TIMESTAMPTZ   NOT NULL DEFAULT now(),
    CONSTRAINT pk_secrets PRIMARY KEY (name)
);

CREATE TABLE IF NOT EXISTS endpoints (
    name              TEXT          NOT NULL,
    endpoint_type     TEXT          NOT NULL,
    payload_spec_ref  TEXT,
    config_ref        TEXT,
    spec              JSONB         NOT NULL,
    retry_policy      JSONB,
    created_at        TIMESTAMPTZ   NOT NULL DEFAULT now(),
    updated_at        TIMESTAMPTZ   NOT NULL DEFAULT now(),
    CONSTRAINT pk_endpoints PRIMARY KEY (name),
    CONSTRAINT fk_endpoints_payload_spec FOREIGN KEY (payload_spec_ref) REFERENCES payload_specs (name),
    CONSTRAINT fk_endpoints_config FOREIGN KEY (config_ref) REFERENCES configs (name),
    CONSTRAINT chk_endpoint_type CHECK (endpoint_type IN ('HTTP', 'KAFKA', 'REDIS_STREAM'))
);

CREATE INDEX IF NOT EXISTS idx_endpoints_type ON endpoints (endpoint_type);

CREATE TABLE IF NOT EXISTS jobs (
    job_id                TEXT          NOT NULL DEFAULT gen_random_uuid()::TEXT,
    endpoint              TEXT          NOT NULL,
    endpoint_type         TEXT          NOT NULL,
    trigger_type          TEXT          NOT NULL,
    status                TEXT          NOT NULL DEFAULT 'ACTIVE',
    version               BIGINT        NOT NULL DEFAULT 1,
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
    created_at            TIMESTAMPTZ   NOT NULL DEFAULT now(),
    retired_at            TIMESTAMPTZ,
    CONSTRAINT pk_jobs PRIMARY KEY (job_id),
    CONSTRAINT fk_jobs_endpoint FOREIGN KEY (endpoint) REFERENCES endpoints (name),
    CONSTRAINT chk_trigger_type CHECK (trigger_type IN ('IMMEDIATE', 'DELAYED', 'CRON')),
    CONSTRAINT chk_status CHECK (status IN ('ACTIVE', 'RETIRED')),
    CONSTRAINT chk_endpoint_type CHECK (endpoint_type IN ('HTTP', 'KAFKA', 'REDIS_STREAM'))
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_jobs_idempotency
    ON jobs (endpoint, idempotency_key)
    WHERE idempotency_key IS NOT NULL;

CREATE INDEX IF NOT EXISTS idx_jobs_cron_due
    ON jobs (cron_next_run_at)
    WHERE trigger_type = 'CRON' AND status = 'ACTIVE';

CREATE INDEX IF NOT EXISTS idx_jobs_endpoint
    ON jobs (endpoint, created_at DESC);

CREATE INDEX IF NOT EXISTS idx_jobs_status
    ON jobs (status, created_at DESC);

CREATE TABLE IF NOT EXISTS executions (
    execution_id    TEXT          NOT NULL DEFAULT gen_random_uuid()::TEXT,
    job_id          TEXT          NOT NULL,
    endpoint        TEXT          NOT NULL,
    endpoint_type   TEXT          NOT NULL,
    idempotency_key TEXT,
    status          TEXT          NOT NULL DEFAULT 'PENDING',
    input           JSONB,
    output          JSONB,
    attempt_count   BIGINT        NOT NULL DEFAULT 0,
    max_attempts    BIGINT        NOT NULL DEFAULT 1,
    worker_id       TEXT,
    run_at          TIMESTAMPTZ   NOT NULL DEFAULT now(),
    started_at      TIMESTAMPTZ,
    completed_at    TIMESTAMPTZ,
    duration_ms     BIGINT,
    created_at      TIMESTAMPTZ   NOT NULL DEFAULT now(),
    CONSTRAINT pk_executions PRIMARY KEY (execution_id),
    CONSTRAINT fk_executions_job FOREIGN KEY (job_id) REFERENCES jobs (job_id),
    CONSTRAINT chk_exec_status CHECK (status IN (
        'PENDING', 'QUEUED', 'RUNNING', 'RETRYING', 'SUCCESS', 'FAILED', 'CANCELLED'
    ))
);

CREATE INDEX IF NOT EXISTS idx_executions_pickup
    ON executions (status, run_at ASC)
    WHERE status IN ('QUEUED', 'RETRYING');

CREATE UNIQUE INDEX IF NOT EXISTS idx_executions_cron_dedup
    ON executions (job_id, idempotency_key)
    WHERE idempotency_key IS NOT NULL;

CREATE INDEX IF NOT EXISTS idx_executions_by_job
    ON executions (job_id, created_at DESC);

CREATE INDEX IF NOT EXISTS idx_executions_running
    ON executions (status, started_at)
    WHERE status = 'RUNNING';

CREATE TABLE IF NOT EXISTS attempts (
    attempt_id      TEXT          NOT NULL DEFAULT gen_random_uuid()::TEXT,
    execution_id    TEXT          NOT NULL,
    attempt_number  BIGINT        NOT NULL,
    status          TEXT          NOT NULL,
    started_at      TIMESTAMPTZ   NOT NULL,
    completed_at    TIMESTAMPTZ,
    duration_ms     BIGINT,
    output          JSONB,
    error           JSONB,
    created_at      TIMESTAMPTZ   NOT NULL DEFAULT now(),
    CONSTRAINT pk_attempts PRIMARY KEY (attempt_id),
    CONSTRAINT fk_attempts_execution FOREIGN KEY (execution_id) REFERENCES executions (execution_id),
    CONSTRAINT uq_attempts_exec_number UNIQUE (execution_id, attempt_number),
    CONSTRAINT chk_attempt_status CHECK (status IN ('SUCCESS', 'FAILED'))
);

CREATE INDEX IF NOT EXISTS idx_attempts_by_execution
    ON attempts (execution_id, attempt_number ASC);

CREATE TABLE IF NOT EXISTS execution_logs (
    log_id          TEXT          NOT NULL DEFAULT gen_random_uuid()::TEXT,
    execution_id    TEXT          NOT NULL,
    attempt_number  BIGINT        NOT NULL,
    level           TEXT          NOT NULL,
    message         TEXT          NOT NULL,
    logged_at       TIMESTAMPTZ   NOT NULL DEFAULT now(),
    CONSTRAINT pk_execution_logs PRIMARY KEY (log_id),
    CONSTRAINT fk_logs_execution FOREIGN KEY (execution_id) REFERENCES executions (execution_id),
    CONSTRAINT chk_log_level CHECK (level IN ('DEBUG', 'INFO', 'WARN', 'ERROR'))
);

CREATE INDEX IF NOT EXISTS idx_logs_by_execution
    ON execution_logs (execution_id, logged_at ASC);

CREATE INDEX IF NOT EXISTS idx_logs_by_attempt
    ON execution_logs (execution_id, attempt_number, logged_at ASC);

CREATE TABLE IF NOT EXISTS region_heartbeats (
    region        TEXT          NOT NULL,
    component     TEXT          NOT NULL,
    last_beat_at  TIMESTAMPTZ   NOT NULL DEFAULT now(),
    status        TEXT          NOT NULL DEFAULT 'ALIVE',
    metadata      JSONB,
    CONSTRAINT pk_region_heartbeats PRIMARY KEY (region, component)
);

CREATE TABLE IF NOT EXISTS region_status (
    region        TEXT          NOT NULL,
    alive         BOOL          NOT NULL DEFAULT true,
    failed_at     TIMESTAMPTZ,
    adopted_by    TEXT,
    updated_at    TIMESTAMPTZ   NOT NULL DEFAULT now(),
    CONSTRAINT pk_region_status PRIMARY KEY (region)
);

INSERT INTO region_status (region, alive, updated_at) VALUES ('default', true, now())
ON CONFLICT (region) DO NOTHING;
