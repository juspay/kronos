-- Long-running jobs: WAITING / POLLING execution statuses, polls table,
-- per-execution and per-job async bounds.

-- Extend executions status CHECK
ALTER TABLE {p}executions DROP CONSTRAINT chk_{p}exec_status;
ALTER TABLE {p}executions ADD CONSTRAINT chk_{p}exec_status CHECK (status IN (
    'PENDING', 'QUEUED', 'RUNNING', 'RETRYING',
    'SUCCESS', 'FAILED', 'CANCELLED',
    'WAITING', 'POLLING'
));

-- A dispatch that parks the execution records its attempt as WAITING
ALTER TABLE {p}attempts DROP CONSTRAINT chk_{p}attempt_status;
ALTER TABLE {p}attempts ADD CONSTRAINT chk_{p}attempt_status CHECK (status IN (
    'SUCCESS', 'FAILED', 'WAITING'
));

-- Long-running columns on executions (snapshot of effective values + runtime state)
ALTER TABLE {p}executions
    ADD COLUMN poll_url            TEXT,
    ADD COLUMN poll_count          INT         NOT NULL DEFAULT 0,
    ADD COLUMN polling_started_at  TIMESTAMPTZ,
    ADD COLUMN polling_deadline    TIMESTAMPTZ,
    ADD COLUMN max_wait_ms         BIGINT,
    ADD COLUMN max_polls           INT;

-- Extend pickup index to include WAITING
DROP INDEX IF EXISTS idx_{p}executions_pickup;
CREATE INDEX idx_{p}executions_pickup
    ON {p}executions (status, run_at ASC)
    WHERE status IN ('QUEUED', 'RETRYING', 'PENDING', 'WAITING');

-- Per-job async overrides (resolved at job creation; copied to executions on insert)
ALTER TABLE {p}jobs
    ADD COLUMN async_max_wait_ms   BIGINT,
    ADD COLUMN async_max_polls     INT;

-- polls table mirrors attempts in shape
CREATE TABLE IF NOT EXISTS {p}polls (
    execution_id    TEXT        NOT NULL,
    poll_number     INT         NOT NULL,
    polled_at       TIMESTAMPTZ NOT NULL,
    duration_ms     BIGINT,
    status_code     INT,
    retry_after_ms  BIGINT,
    classification  TEXT        NOT NULL,
    error           JSONB,
    CONSTRAINT pk_{p}polls PRIMARY KEY (execution_id, poll_number),
    CONSTRAINT fk_{p}polls_execution FOREIGN KEY (execution_id) REFERENCES {p}executions (execution_id),
    CONSTRAINT chk_{p}poll_classification CHECK (classification IN (
        'SUCCESS', 'PENDING', 'TERMINAL_FAILURE', 'TRANSIENT_ERROR'
    ))
);
