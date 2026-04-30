-- Migration: Replace CRON materializer with pg_cron
-- pg_cron handles scheduling natively, eliminating the need for the CRON materializer loop.

CREATE EXTENSION IF NOT EXISTS pg_cron;

-- Migrate existing active CRON jobs to pg_cron.
-- For each active workspace, register all active CRON jobs with pg_cron.
DO $$ DECLARE
    ws RECORD;
    job RECORD;
    cron_job_name TEXT;
    cron_command TEXT;
BEGIN
    FOR ws IN SELECT schema_name FROM {{system_schema}}.workspaces WHERE status = 'ACTIVE' LOOP
        FOR job IN EXECUTE format(
            'SELECT job_id, cron_expression, endpoint FROM %I.jobs WHERE trigger_type = ''CRON'' AND status = ''ACTIVE''',
            ws.schema_name
        ) LOOP
            cron_job_name := 'kronos_' || ws.schema_name || '_' || job.job_id;
            cron_command := format(
                'INSERT INTO %I.executions '
                    '(job_id, endpoint, endpoint_type, idempotency_key, status, input, run_at, max_attempts) '
                'SELECT j.job_id, j.endpoint, j.endpoint_type, '
                    '''cron_'' || j.job_id || ''_'' || (EXTRACT(EPOCH FROM now()) * 1000)::BIGINT, '
                    '''QUEUED'', j.input, now(), '
                    'COALESCE((e.retry_policy->>''max_attempts'')::BIGINT, 1) '
                'FROM %I.jobs j '
                'JOIN %I.endpoints e ON e.name = j.endpoint '
                'WHERE j.job_id = %L AND j.status = ''ACTIVE'' '
                'ON CONFLICT (job_id, idempotency_key) WHERE idempotency_key IS NOT NULL DO NOTHING',
                ws.schema_name, ws.schema_name, ws.schema_name, job.job_id
            );

            PERFORM cron.schedule(cron_job_name, job.cron_expression, cron_command);
        END LOOP;
    END LOOP;
END $$;
