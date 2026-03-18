use crate::models::Endpoint;
use sqlx::PgPool;

pub async fn create(
    pool: &PgPool,
    name: &str,
    endpoint_type: &str,
    payload_spec_ref: Option<&str>,
    config_ref: Option<&str>,
    spec: &serde_json::Value,
    retry_policy: Option<&serde_json::Value>,
) -> Result<Endpoint, sqlx::Error> {
    sqlx::query_as::<_, Endpoint>(
        "INSERT INTO endpoints (name, endpoint_type, payload_spec_ref, config_ref, spec, retry_policy)
         VALUES ($1, $2, $3, $4, $5, $6)
         RETURNING name, endpoint_type, payload_spec_ref, config_ref, spec, retry_policy, created_at, updated_at"
    )
    .bind(name)
    .bind(endpoint_type)
    .bind(payload_spec_ref)
    .bind(config_ref)
    .bind(spec)
    .bind(retry_policy)
    .fetch_one(pool)
    .await
}

pub async fn get(pool: &PgPool, name: &str) -> Result<Option<Endpoint>, sqlx::Error> {
    sqlx::query_as::<_, Endpoint>(
        "SELECT name, endpoint_type, payload_spec_ref, config_ref, spec, retry_policy, created_at, updated_at
         FROM endpoints WHERE name = $1"
    )
    .bind(name)
    .fetch_optional(pool)
    .await
}

pub async fn list(
    pool: &PgPool,
    cursor: Option<&str>,
    limit: i64,
) -> Result<Vec<Endpoint>, sqlx::Error> {
    match cursor {
        Some(c) => {
            sqlx::query_as::<_, Endpoint>(
                "SELECT name, endpoint_type, payload_spec_ref, config_ref, spec, retry_policy, created_at, updated_at
                 FROM endpoints WHERE name > $1 ORDER BY name ASC LIMIT $2"
            )
            .bind(c)
            .bind(limit)
            .fetch_all(pool)
            .await
        }
        None => {
            sqlx::query_as::<_, Endpoint>(
                "SELECT name, endpoint_type, payload_spec_ref, config_ref, spec, retry_policy, created_at, updated_at
                 FROM endpoints ORDER BY name ASC LIMIT $1"
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
    spec: Option<&serde_json::Value>,
    config_ref: Option<&str>,
    payload_spec_ref: Option<&str>,
    retry_policy: Option<&serde_json::Value>,
) -> Result<Option<Endpoint>, sqlx::Error> {
    // Build dynamic update - for simplicity, update all provided fields
    sqlx::query_as::<_, Endpoint>(
        "UPDATE endpoints SET
            spec = COALESCE($2, spec),
            config_ref = COALESCE($3, config_ref),
            payload_spec_ref = COALESCE($4, payload_spec_ref),
            retry_policy = COALESCE($5, retry_policy),
            updated_at = now()
         WHERE name = $1
         RETURNING name, endpoint_type, payload_spec_ref, config_ref, spec, retry_policy, created_at, updated_at"
    )
    .bind(name)
    .bind(spec)
    .bind(config_ref)
    .bind(payload_spec_ref)
    .bind(retry_policy)
    .fetch_optional(pool)
    .await
}

pub async fn delete(pool: &PgPool, name: &str) -> Result<bool, sqlx::Error> {
    let result = sqlx::query("DELETE FROM endpoints WHERE name = $1")
        .bind(name)
        .execute(pool)
        .await?;
    Ok(result.rows_affected() > 0)
}

pub async fn has_active_jobs(pool: &PgPool, name: &str) -> Result<bool, sqlx::Error> {
    let row: (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM jobs WHERE endpoint = $1 AND status = 'ACTIVE'")
            .bind(name)
            .fetch_one(pool)
            .await?;
    Ok(row.0 > 0)
}
