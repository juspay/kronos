-- Migration: enforce cron_ends_at for pg_cron-driven CRON jobs.
--
-- The original pg_cron registration (20260322000001) materialized executions
-- with only a `status = 'ACTIVE'` guard, so jobs kept firing forever, ignoring
-- their `cron_ends_at` window. This migration repairs jobs already registered:
--   1. Expired active CRON jobs are retired and unscheduled from pg_cron.
--   2. Still-active CRON jobs are re-registered with the `cron_ends_at` guard,
--      matching db::jobs::build_cron_command in the application.
--
-- cron.schedule(name, ...) upserts by job name, so re-registering replaces the
-- old command in place.

DO $$ DECLARE
    ws RECORD;
    job RECORD;
    cron_job_name TEXT;
    cron_command TEXT;
BEGIN
    FOR ws IN SELECT schema_name FROM public.workspaces WHERE status = 'ACTIVE' LOOP

        -- 1. Retire + unschedule CRON jobs whose end window has already passed.
        FOR job IN EXECUTE format(
            'SELECT job_id FROM %I.jobs '
            'WHERE trigger_type = ''CRON'' AND status = ''ACTIVE'' '
              'AND cron_ends_at IS NOT NULL AND cron_ends_at <= now()',
            ws.schema_name
        ) LOOP
            EXECUTE format(
                'UPDATE %I.jobs SET status = ''RETIRED'', retired_at = now() WHERE job_id = %L',
                ws.schema_name, job.job_id
            );
            cron_job_name := 'kronos_' || ws.schema_name || '_' || job.job_id;
            -- cron.unschedule raises if the entry is missing; ignore that.
            BEGIN
                PERFORM cron.unschedule(cron_job_name);
            EXCEPTION WHEN OTHERS THEN
                NULL;
            END;
        END LOOP;

        -- 2. Re-register the still-active CRON jobs with the ends_at-guarded command.
        FOR job IN EXECUTE format(
            'SELECT job_id, cron_expression FROM %I.jobs '
            'WHERE trigger_type = ''CRON'' AND status = ''ACTIVE''',
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
                    'AND (j.cron_ends_at IS NULL OR j.cron_ends_at > now()) '
                'ON CONFLICT (job_id, idempotency_key) WHERE idempotency_key IS NOT NULL DO NOTHING',
                ws.schema_name, ws.schema_name, ws.schema_name, job.job_id
            );

            PERFORM cron.schedule(cron_job_name, job.cron_expression, cron_command);
        END LOOP;

    END LOOP;
END $$;
