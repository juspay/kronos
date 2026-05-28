-- Migration: dogfood the CRON reaper through kronos itself.
--
-- The reaper used to run as a tokio interval task inside each worker pod —
-- invisible to the dashboard, to the executions table, and to the same metrics
-- pipeline we run for user jobs. This migration lifts it to a first-class
-- kronos CRON job:
--
--   1. Allow a new `INTERNAL` endpoint type alongside HTTP/KAFKA/REDIS_STREAM.
--      INTERNAL endpoints route to an in-process dispatcher in the worker
--      (see `dispatcher::internal`) that runs a named task ({"task":"reaper"})
--      rather than performing an external dispatch.
--
--   2. For every existing active workspace, register a `kronos.reaper`
--      INTERNAL endpoint and a CRON job firing every minute, mirroring what
--      `worker::bootstrap` does for newly-provisioned workspaces. pg_cron
--      materializes one execution per tick into the workspace's own executions
--      table, the worker picks it up via the normal SKIP LOCKED claim, and the
--      sweep runs as the execution's "dispatch".
--
-- The new (jobs|endpoints).chk_endpoint_type constraints are dropped first
-- because Postgres rejects ALTER ... ADD CONSTRAINT for an existing name;
-- DROP IF EXISTS keeps the migration replayable on fresh schemas where the
-- newer workspace_v1.sql already includes INTERNAL.

DO $$ DECLARE
    ws RECORD;
    reaper_job_id TEXT;
    cron_job_name TEXT;
    cron_command TEXT;
BEGIN
    FOR ws IN SELECT schema_name FROM public.workspaces WHERE status = 'ACTIVE' LOOP

        -- 1. Widen endpoint_type check constraints to include INTERNAL.
        EXECUTE format(
            'ALTER TABLE %I.endpoints DROP CONSTRAINT IF EXISTS chk_endpoint_type',
            ws.schema_name
        );
        EXECUTE format(
            'ALTER TABLE %I.endpoints ADD CONSTRAINT chk_endpoint_type '
            'CHECK (endpoint_type IN (''HTTP'', ''KAFKA'', ''REDIS_STREAM'', ''INTERNAL''))',
            ws.schema_name
        );
        EXECUTE format(
            'ALTER TABLE %I.jobs DROP CONSTRAINT IF EXISTS chk_endpoint_type',
            ws.schema_name
        );
        EXECUTE format(
            'ALTER TABLE %I.jobs ADD CONSTRAINT chk_endpoint_type '
            'CHECK (endpoint_type IN (''HTTP'', ''KAFKA'', ''REDIS_STREAM'', ''INTERNAL''))',
            ws.schema_name
        );

        -- 2. Provision the reaper endpoint (idempotent — primary key is name).
        EXECUTE format(
            'INSERT INTO %I.endpoints (name, endpoint_type, spec) '
            'VALUES (''kronos.reaper'', ''INTERNAL'', ''{"task":"reaper"}''::jsonb) '
            'ON CONFLICT (name) DO NOTHING',
            ws.schema_name
        );

        -- 3. Provision the reaper CRON job. idempotency_key=''kronos.reaper''
        -- pins it to the partial unique index on (endpoint, idempotency_key),
        -- so concurrent migrations across replicas can''t double-insert.
        EXECUTE format(
            'INSERT INTO %I.jobs ('
                'endpoint, endpoint_type, trigger_type, idempotency_key, '
                'cron_expression, cron_timezone, cron_next_run_at'
            ') VALUES ('
                '''kronos.reaper'', ''INTERNAL'', ''CRON'', ''kronos.reaper'', '
                '''* * * * *'', ''UTC'', now()'
            ') ON CONFLICT (endpoint, idempotency_key) '
            'WHERE idempotency_key IS NOT NULL DO NOTHING '
            'RETURNING job_id',
            ws.schema_name
        ) INTO reaper_job_id;

        -- If the insert was a no-op, fetch the existing job_id so we can still
        -- ensure pg_cron has a matching entry (it may have been pruned).
        IF reaper_job_id IS NULL THEN
            EXECUTE format(
                'SELECT job_id FROM %I.jobs '
                'WHERE endpoint = ''kronos.reaper'' AND status = ''ACTIVE'' '
                'LIMIT 1',
                ws.schema_name
            ) INTO reaper_job_id;
        END IF;

        IF reaper_job_id IS NULL THEN
            CONTINUE;
        END IF;

        -- 4. Register the pg_cron entry. cron.schedule upserts by name, so this
        -- is safe to re-run; the command mirrors db::jobs::build_cron_command.
        cron_job_name := 'kronos_' || ws.schema_name || '_' || reaper_job_id;
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
            ws.schema_name, ws.schema_name, ws.schema_name, reaper_job_id
        );

        PERFORM cron.schedule(cron_job_name, '* * * * *', cron_command);

    END LOOP;
END $$;
