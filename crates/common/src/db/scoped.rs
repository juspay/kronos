use crate::tenant::validate_schema_name;
use sqlx::PgPool;

/// Acquire a connection with search_path set to the workspace schema.
pub async fn scoped_connection(
    pool: &PgPool,
    schema_name: &str,
) -> Result<sqlx::pool::PoolConnection<sqlx::Postgres>, sqlx::Error> {
    assert!(
        validate_schema_name(schema_name),
        "Invalid schema name: {}",
        schema_name
    );
    let mut conn = pool.acquire().await?;
    // Strict workspace scope, no `public` fallback: a missing schema must error
    // rather than silently read public (control tables use `public.*` directly).
    let set_path = format!("SET search_path TO \"{}\"", schema_name);
    sqlx::query(&set_path).execute(&mut *conn).await?;
    Ok(conn)
}

/// Begin a transaction with search_path set to the workspace schema.
pub async fn scoped_transaction<'a>(
    pool: &'a PgPool,
    schema_name: &str,
) -> Result<sqlx::Transaction<'a, sqlx::Postgres>, sqlx::Error> {
    assert!(
        validate_schema_name(schema_name),
        "Invalid schema name: {}",
        schema_name
    );
    let mut tx = pool.begin().await?;
    // Strict workspace scope, no `public` fallback (see `scoped_connection`).
    let set_path = format!("SET search_path TO \"{}\"", schema_name);
    sqlx::query(&set_path).execute(&mut *tx).await?;
    Ok(tx)
}
