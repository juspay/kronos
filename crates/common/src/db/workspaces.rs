use crate::db::jobs::register_pg_cron;
use crate::db::scoped::scoped_transaction;
use crate::models::workspace::Workspace;
use crate::tenant::validate_schema_name;
use sqlx::PgPool;

const WORKSPACE_SCHEMA_V1: &str = include_str!("../../../../migrations/workspace_v1.sql");

/// Identifiers for the dogfooded reaper kronos installs in every workspace.
/// The reaper is an `INTERNAL` CRON job whose ticks materialize executions
/// into the workspace's own `executions` table — see `worker::dispatcher::internal`.
const REAPER_ENDPOINT_NAME: &str = "kronos.reaper";
/// pg_cron's 5-field expressions max out at minute granularity, so the reaper
/// runs once a minute. Plenty for a lifecycle sweep — the `cron_ends_at` guard
/// in `build_cron_command` already stops new executions immediately.
const REAPER_CRON_EXPRESSION: &str = "* * * * *";

pub async fn create(
    pool: &PgPool,
    org_id: &str,
    name: &str,
    slug: &str,
    schema_name: &str,
) -> Result<Workspace, sqlx::Error> {
    assert!(
        validate_schema_name(schema_name),
        "Invalid schema name: {}",
        schema_name
    );

    let workspace = sqlx::query_as::<_, Workspace>(
        "INSERT INTO public.workspaces (org_id, name, slug, schema_name)
         VALUES ($1, $2, $3, $4)
         RETURNING *",
    )
    .bind(org_id)
    .bind(name)
    .bind(slug)
    .bind(schema_name)
    .fetch_one(pool)
    .await?;

    // Create the schema and apply workspace DDL
    provision_schema(pool, schema_name).await?;

    // Install kronos's own dogfooded reaper into this workspace. Done as part
    // of provisioning rather than from a background loop, so a freshly-created
    // workspace has its reaper job ready by the time `create` returns. If this
    // fails the workspace row exists but schema_version stays unset, marking
    // it as half-provisioned for the operator to investigate.
    provision_reaper(pool, schema_name).await?;

    // Update schema_version
    sqlx::query(
        "UPDATE public.workspaces SET schema_version = 1 WHERE workspace_id = $1",
    )
    .bind(&workspace.workspace_id)
    .execute(pool)
    .await?;

    // Re-fetch to get updated schema_version
    Ok(sqlx::query_as::<_, Workspace>(
        "SELECT * FROM public.workspaces WHERE workspace_id = $1",
    )
    .bind(&workspace.workspace_id)
    .fetch_one(pool)
    .await?)
}

pub async fn get(pool: &PgPool, workspace_id: &str) -> Result<Option<Workspace>, sqlx::Error> {
    sqlx::query_as::<_, Workspace>(
        "SELECT * FROM public.workspaces WHERE workspace_id = $1",
    )
    .bind(workspace_id)
    .fetch_optional(pool)
    .await
}

pub async fn get_by_org_and_id(
    pool: &PgPool,
    org_id: &str,
    workspace_id: &str,
) -> Result<Option<Workspace>, sqlx::Error> {
    sqlx::query_as::<_, Workspace>(
        "SELECT * FROM public.workspaces WHERE org_id = $1 AND workspace_id = $2",
    )
    .bind(org_id)
    .bind(workspace_id)
    .fetch_optional(pool)
    .await
}

pub async fn list_for_org(
    pool: &PgPool,
    org_id: &str,
) -> Result<Vec<Workspace>, sqlx::Error> {
    sqlx::query_as::<_, Workspace>(
        "SELECT * FROM public.workspaces WHERE org_id = $1 AND status = 'ACTIVE'
         ORDER BY created_at DESC",
    )
    .bind(org_id)
    .fetch_all(pool)
    .await
}

pub async fn resolve_schema(
    pool: &PgPool,
    org_id: &str,
    workspace_id: &str,
) -> Result<Option<String>, sqlx::Error> {
    let row: Option<(String,)> = sqlx::query_as(
        "SELECT schema_name FROM public.workspaces
         WHERE org_id = $1 AND workspace_id = $2 AND status = 'ACTIVE'",
    )
    .bind(org_id)
    .bind(workspace_id)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|r| r.0))
}

async fn provision_schema(pool: &PgPool, schema_name: &str) -> Result<(), sqlx::Error> {
    let create_schema = format!("CREATE SCHEMA IF NOT EXISTS \"{}\"", schema_name);
    sqlx::query(&create_schema).execute(pool).await?;

    // Run workspace DDL within the new schema using raw_sql which supports multiple statements
    let ddl = format!(
        "SET search_path TO \"{schema_name}\"; {WORKSPACE_SCHEMA_V1} SET search_path TO public;"
    );
    sqlx::raw_sql(&ddl).execute(pool).await?;

    Ok(())
}

/// Install the dogfooded reaper for a freshly-provisioned workspace: an
/// `INTERNAL` endpoint, a `* * * * *` CRON job, and the matching pg_cron entry
/// that materializes one execution per minute. The endpoint + job inserts run
/// in a single scoped transaction so a partial provisioning never leaves an
/// endpoint without its job (or vice versa); the pg_cron registration follows
/// because `cron.schedule` writes to the `cron` schema and so can't share the
/// workspace tx.
async fn provision_reaper(pool: &PgPool, schema_name: &str) -> Result<(), sqlx::Error> {
    let mut tx = scoped_transaction(pool, schema_name).await?;

    sqlx::query(
        "INSERT INTO endpoints (name, endpoint_type, spec) \
         VALUES ($1, 'INTERNAL', '{\"task\":\"reaper\"}'::jsonb)",
    )
    .bind(REAPER_ENDPOINT_NAME)
    .execute(&mut *tx)
    .await?;

    let (job_id,): (String,) = sqlx::query_as(
        "INSERT INTO jobs ( \
            endpoint, endpoint_type, trigger_type, \
            cron_expression, cron_timezone, cron_next_run_at \
         ) VALUES ($1, 'INTERNAL', 'CRON', $2, 'UTC', now()) \
         RETURNING job_id",
    )
    .bind(REAPER_ENDPOINT_NAME)
    .bind(REAPER_CRON_EXPRESSION)
    .fetch_one(&mut *tx)
    .await?;

    tx.commit().await?;

    register_pg_cron(pool, schema_name, &job_id, REAPER_CRON_EXPRESSION).await?;

    Ok(())
}
