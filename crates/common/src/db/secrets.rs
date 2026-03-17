use crate::models::secret::Secret;
use sqlx::PgPool;

pub async fn create(pool: &PgPool, name: &str, encrypted_value: &[u8]) -> Result<Secret, sqlx::Error> {
    sqlx::query_as::<_, Secret>(
        "INSERT INTO secrets (name, encrypted_value) VALUES ($1, $2)
         RETURNING name, encrypted_value, created_at, updated_at"
    )
    .bind(name)
    .bind(encrypted_value)
    .fetch_one(pool)
    .await
}

pub async fn get(pool: &PgPool, name: &str) -> Result<Option<Secret>, sqlx::Error> {
    sqlx::query_as::<_, Secret>(
        "SELECT name, encrypted_value, created_at, updated_at FROM secrets WHERE name = $1"
    )
    .bind(name)
    .fetch_optional(pool)
    .await
}

pub async fn list(pool: &PgPool, cursor: Option<&str>, limit: i64) -> Result<Vec<Secret>, sqlx::Error> {
    match cursor {
        Some(c) => {
            sqlx::query_as::<_, Secret>(
                "SELECT name, encrypted_value, created_at, updated_at FROM secrets
                 WHERE name > $1 ORDER BY name ASC LIMIT $2"
            )
            .bind(c)
            .bind(limit)
            .fetch_all(pool)
            .await
        }
        None => {
            sqlx::query_as::<_, Secret>(
                "SELECT name, encrypted_value, created_at, updated_at FROM secrets
                 ORDER BY name ASC LIMIT $1"
            )
            .bind(limit)
            .fetch_all(pool)
            .await
        }
    }
}

pub async fn update(pool: &PgPool, name: &str, encrypted_value: &[u8]) -> Result<Option<Secret>, sqlx::Error> {
    sqlx::query_as::<_, Secret>(
        "UPDATE secrets SET encrypted_value = $2, updated_at = now()
         WHERE name = $1
         RETURNING name, encrypted_value, created_at, updated_at"
    )
    .bind(name)
    .bind(encrypted_value)
    .fetch_optional(pool)
    .await
}

pub async fn delete(pool: &PgPool, name: &str) -> Result<bool, sqlx::Error> {
    let result = sqlx::query("DELETE FROM secrets WHERE name = $1")
        .bind(name)
        .execute(pool)
        .await?;
    Ok(result.rows_affected() > 0)
}

pub async fn has_dependent_endpoints(pool: &PgPool, name: &str) -> Result<bool, sqlx::Error> {
    // Secrets are referenced in endpoint spec templates, not via FK.
    // For now, check if any endpoint spec contains the secret name in a template.
    let row: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM endpoints WHERE spec::TEXT LIKE '%{{secret.' || $1 || '}}%'"
    )
    .bind(name)
    .fetch_one(pool)
    .await?;
    Ok(row.0 > 0)
}
