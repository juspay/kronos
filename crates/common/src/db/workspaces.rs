use crate::models::workspace::Workspace;
use crate::tenant::validate_schema_name;
use sqlx::PgPool;

const WORKSPACE_SCHEMA_V1: &str = include_str!("../../../../migrations/workspace_v1.sql");

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
