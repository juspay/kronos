use crate::db::jobs::register_pg_cron_conn;
use crate::db::scoped::scoped_transaction;
use crate::models::endpoint::EndpointType;
use crate::models::job::TriggerType;
use crate::models::workspace::Workspace;
use crate::tenant::validate_schema_name;
use sqlx::PgPool;

const WORKSPACE_SCHEMA_V1: &str = include_str!("../../migrations/workspace_v1.sql");

/// Endpoint name kronos installs in every workspace for its dogfooded reaper.
/// The reaper is an `INTERNAL` CRON job whose ticks materialize executions
/// into the workspace's own `executions` table — see `worker::dispatcher::internal`.
const REAPER_ENDPOINT_NAME: &str = "kronos.reaper";

pub async fn create(
    pool: &PgPool,
    org_id: &str,
    name: &str,
    slug: &str,
    schema_name: &str,
    reaper_cron_expression: &str,
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
    provision_schema(pool, schema_name, "").await?;

    // Install kronos's own dogfooded reaper into this workspace. Done as part
    // of provisioning rather than from a background loop, so a freshly-created
    // workspace has its reaper job ready by the time `create` returns. If this
    // fails the workspace row exists but schema_version stays unset, marking
    // it as half-provisioned for the operator to investigate.
    // The API deployment runs schema-per-workspace with unprefixed tables.
    provision_reaper(pool, schema_name, "", reaper_cron_expression).await?;

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

/// Resolve a workspace by its org reference and workspace reference, where each
/// reference may be the UUID id (`org_id` / `workspace_id`) or the human-readable
/// `slug`. Exact id matches are preferred over slug matches.
pub async fn get_by_org_and_id(
    pool: &PgPool,
    org_ref: &str,
    workspace_ref: &str,
) -> Result<Option<Workspace>, sqlx::Error> {
    sqlx::query_as::<_, Workspace>(
        "SELECT w.* FROM public.workspaces w
         JOIN public.organizations o ON o.org_id = w.org_id
         WHERE (o.org_id = $1 OR o.slug = $1)
           AND (w.workspace_id = $2 OR w.slug = $2)
         ORDER BY (o.org_id = $1) DESC, (w.workspace_id = $2) DESC
         LIMIT 1",
    )
    .bind(org_ref)
    .bind(workspace_ref)
    .fetch_optional(pool)
    .await
}

/// List active workspaces for an org addressed by its `org_id` or its `slug`.
pub async fn list_for_org(
    pool: &PgPool,
    org_ref: &str,
) -> Result<Vec<Workspace>, sqlx::Error> {
    sqlx::query_as::<_, Workspace>(
        "SELECT w.* FROM public.workspaces w
         JOIN public.organizations o ON o.org_id = w.org_id
         WHERE (o.org_id = $1 OR o.slug = $1) AND w.status = 'ACTIVE'
         ORDER BY w.created_at DESC",
    )
    .bind(org_ref)
    .fetch_all(pool)
    .await
}

/// Resolve the tenant schema name for an org/workspace pair, where each side may
/// be addressed by id or slug. Exact id matches are preferred.
pub async fn resolve_schema(
    pool: &PgPool,
    org_ref: &str,
    workspace_ref: &str,
) -> Result<Option<String>, sqlx::Error> {
    let row: Option<(String,)> = sqlx::query_as(
        "SELECT w.schema_name FROM public.workspaces w
         JOIN public.organizations o ON o.org_id = w.org_id
         WHERE (o.org_id = $1 OR o.slug = $1)
           AND (w.workspace_id = $2 OR w.slug = $2)
           AND w.status = 'ACTIVE'
         ORDER BY (o.org_id = $1) DESC, (w.workspace_id = $2) DESC
         LIMIT 1",
    )
    .bind(org_ref)
    .bind(workspace_ref)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|r| r.0))
}

pub async fn provision_schema(
    pool: &PgPool,
    schema_name: &str,
    table_prefix: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(&format!("CREATE SCHEMA IF NOT EXISTS \"{}\"", schema_name))
        .execute(pool)
        .await?;

    let mut conn = crate::db::scoped::scoped_connection(pool, schema_name).await?;

    // `table_prefix` is used as-is: callers pass the full prefix including any trailing
    // separator (e.g. "kronos_"), matching the read side (`tbl(prefix, name)` =
    // `{prefix}{name}`). Do NOT append an underscore here, or provisioned tables
    // (`kronos__endpoints`) won't match the names queried at runtime (`kronos_endpoints`).
    let ddl = WORKSPACE_SCHEMA_V1.replace("{p}", table_prefix);
    for stmt in ddl.split(';') {
        let stmt = stmt.trim();
        if !stmt.is_empty() {
            sqlx::query(stmt).execute(&mut *conn).await?;
        }
    }

    sqlx::query("SET search_path TO public")
        .execute(&mut *conn)
        .await?;

    Ok(())
}

/// Install the dogfooded reaper for a freshly-provisioned workspace: an
/// `INTERNAL` endpoint, a CRON job firing on `cron_expression`, and the
/// matching pg_cron entry that materializes one execution per tick. All three
/// run inside the same scoped transaction so a failure at any step rolls the
/// whole provisioning back — we never commit a job row without its pg_cron
/// schedule (which would leave a phantom row that never fires).
///
/// The reaper is what gives `cron_ends_at` any effect on a job's *lifecycle*:
/// pg_cron has no concept of an end date, so the guard in the pg_cron command
/// stops new executions past the window while the entry keeps ticking and the
/// job stays ACTIVE forever. Each sweep retires expired CRON jobs and removes
/// their pg_cron entries. A workspace without one leaks a pg_cron entry per
/// expired job, permanently.
///
/// `table_prefix` is used as-is (see [`provision_schema`]): the API deployment
/// passes `""`, library mode passes its configured prefix. Getting this wrong
/// writes the reaper into tables nothing reads.
///
/// Idempotent — an embedder may call `provision_workspace` on every boot, so
/// re-running must not stack a second reaper or fail on the endpoint's primary
/// key.
///
/// `cron_expression` is the caller-supplied schedule (typically from
/// `AppConfig::reaper::cron_expression`); it is validated as a 5-field
/// PgCronExpr at config-load time, so this function trusts it as a literal.
pub async fn provision_reaper(
    pool: &PgPool,
    schema_name: &str,
    table_prefix: &str,
    cron_expression: &str,
) -> Result<(), sqlx::Error> {
    let te = crate::db::tbl(table_prefix, "endpoints");
    let tj = crate::db::tbl(table_prefix, "jobs");

    let mut tx = scoped_transaction(pool, schema_name).await?;

    let reaper_spec = serde_json::json!({ "task": "reaper" });

    sqlx::query(&format!(
        "INSERT INTO {te} (name, endpoint_type, spec) \
         VALUES ($1, $2, $3) \
         ON CONFLICT (name) DO NOTHING"
    ))
    .bind(REAPER_ENDPOINT_NAME)
    .bind(EndpointType::INTERNAL.to_string())
    .bind(&reaper_spec)
    .execute(&mut *tx)
    .await?;

    // `WHERE NOT EXISTS` rather than a blind INSERT: a second call must not add
    // a second sweep. Returns no row when one is already installed.
    let existing: Option<(String,)> = sqlx::query_as(&format!(
        "INSERT INTO {tj} ( \
            endpoint, endpoint_type, trigger_type, \
            cron_expression, cron_timezone, cron_next_run_at \
         ) SELECT $1, $2, $3, $4, 'UTC', now() \
         WHERE NOT EXISTS ( \
            SELECT 1 FROM {tj} WHERE endpoint = $1 AND status = 'ACTIVE' \
         ) \
         RETURNING job_id"
    ))
    .bind(REAPER_ENDPOINT_NAME)
    .bind(EndpointType::INTERNAL.to_string())
    .bind(TriggerType::CRON.as_str())
    .bind(cron_expression)
    .fetch_optional(&mut *tx)
    .await?;

    if let Some((job_id,)) = existing {
        register_pg_cron_conn(&mut tx, table_prefix, schema_name, &job_id, cron_expression).await?;
    }

    tx.commit().await?;

    Ok(())
}
