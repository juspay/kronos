use crate::models::PayloadSpec;
use sqlx::PgPool;

pub async fn create(
    pool: &PgPool,
    name: &str,
    schema: &serde_json::Value,
) -> Result<PayloadSpec, sqlx::Error> {
    sqlx::query_as::<_, PayloadSpec>(
        "INSERT INTO payload_specs (name, schema_json) VALUES ($1, $2)
         RETURNING name, schema_json, created_at, updated_at",
    )
    .bind(name)
    .bind(schema)
    .fetch_one(pool)
    .await
}

pub async fn get(pool: &PgPool, name: &str) -> Result<Option<PayloadSpec>, sqlx::Error> {
    sqlx::query_as::<_, PayloadSpec>(
        "SELECT name, schema_json, created_at, updated_at FROM payload_specs WHERE name = $1",
    )
    .bind(name)
    .fetch_optional(pool)
    .await
}

pub async fn list(
    pool: &PgPool,
    cursor: Option<&str>,
    limit: i64,
) -> Result<Vec<PayloadSpec>, sqlx::Error> {
    match cursor {
        Some(c) => {
            sqlx::query_as::<_, PayloadSpec>(
                "SELECT name, schema_json, created_at, updated_at FROM payload_specs
                 WHERE name > $1 ORDER BY name ASC LIMIT $2",
            )
            .bind(c)
            .bind(limit)
            .fetch_all(pool)
            .await
        }
        None => {
            sqlx::query_as::<_, PayloadSpec>(
                "SELECT name, schema_json, created_at, updated_at FROM payload_specs
                 ORDER BY name ASC LIMIT $1",
            )
            .bind(limit)
            .fetch_all(pool)
            .await
        }
    }
}

pub async fn update(
    pool: &PgPool,
    name: &str,
    schema: &serde_json::Value,
) -> Result<Option<PayloadSpec>, sqlx::Error> {
    sqlx::query_as::<_, PayloadSpec>(
        "UPDATE payload_specs SET schema_json = $2, updated_at = now()
         WHERE name = $1
         RETURNING name, schema_json, created_at, updated_at",
    )
    .bind(name)
    .bind(schema)
    .fetch_optional(pool)
    .await
}

pub async fn delete(pool: &PgPool, name: &str) -> Result<bool, sqlx::Error> {
    let result = sqlx::query("DELETE FROM payload_specs WHERE name = $1")
        .bind(name)
        .execute(pool)
        .await?;
    Ok(result.rows_affected() > 0)
}

pub async fn has_dependent_endpoints(pool: &PgPool, name: &str) -> Result<bool, sqlx::Error> {
    let row: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM endpoints WHERE payload_spec_ref = $1")
        .bind(name)
        .fetch_one(pool)
        .await?;
    Ok(row.0 > 0)
}
