-- Migration: Re-register active CRON jobs with start/end window guards
--
-- The per-tick pg_cron materialization command gained two admission guards after
-- the original pg_cron registration was written:
--   * cron_ends_at   -- stop materializing past the job's end window
--   * cron_starts_at -- hold back materialization until the job's start time
--
-- pg_cron entries registered by an earlier app version -- or by the original
-- `20260322000001_pg_cron.sql` backfill -- still carry the UNGUARDED command, so
-- they keep materializing executions outside the job's [starts_at, ends_at)
-- window. A job created with a future starts_at, for instance, fires on the next
-- matching tick instead of waiting.
--
-- `cron.schedule` upserts by job name, so re-scheduling with the current command
-- simply replaces the stored command in place. This backfill is idempotent and
-- safe to re-run.
--
-- IMPORTANT: the command below is a hand-maintained copy of
-- `kronos_common::db::jobs::build_cron_command`. The two MUST stay in sync -- any
-- change to the materialization SQL there has to be mirrored here (and vice
-- versa), or newly-created and backfilled jobs will diverge.
--
-- Note: like the original pg_cron migration, this targets the standalone
-- deployment where tables are unprefixed. Library-mode (TE_TABLE_PREFIX) embeds
-- manage their own schema and are the host application's responsibility.

DO $$ DECLARE
    ws RECORD;
    job RECORD;
    cron_job_name TEXT;
    cron_command TEXT;
BEGIN
    FOR ws IN SELECT schema_name FROM public.workspaces WHERE status = 'ACTIVE' LOOP
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
                    'AND (j.cron_starts_at IS NULL OR j.cron_starts_at <= now()) '
                    'AND (j.cron_ends_at IS NULL OR j.cron_ends_at > now()) '
                'ON CONFLICT (job_id, idempotency_key) WHERE idempotency_key IS NOT NULL DO NOTHING',
                ws.schema_name, ws.schema_name, ws.schema_name, job.job_id
            );

            PERFORM cron.schedule(cron_job_name, job.cron_expression, cron_command);
        END LOOP;
    END LOOP;
END $$;
