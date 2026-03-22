-- Migration: Transaction-based job pickup
-- Workers now pick up PENDING jobs directly (no Delayed Promoter needed).
-- Update the pickup index to include PENDING status.

-- Drop old index that only covered QUEUED and RETRYING
DROP INDEX IF EXISTS idx_executions_pickup;

-- Create new index including PENDING for direct pickup by workers
CREATE INDEX CONCURRENTLY IF NOT EXISTS idx_executions_pickup
    ON executions (status, run_at ASC)
    WHERE status IN ('QUEUED', 'RETRYING', 'PENDING');
