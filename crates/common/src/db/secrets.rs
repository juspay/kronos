use crate::models::secret::Secret;
use sqlx::PgConnection;

pub async fn create(
    conn: &mut PgConnection,
    name: &str,
    provider: &str,
    reference: &str,
) -> Result<Secret, sqlx::Error> {
    sqlx::query_as::<_, Secret>(
        "INSERT INTO secrets (name, provider, reference) VALUES ($1, $2, $3)
         RETURNING name, provider, reference, created_at, updated_at",
    )
    .bind(name)
    .bind(provider)
    .bind(reference)
    .fetch_one(&mut *conn)
    .await
}

pub async fn get(conn: &mut PgConnection, name: &str) -> Result<Option<Secret>, sqlx::Error> {
    sqlx::query_as::<_, Secret>(
        "SELECT name, provider, reference, created_at, updated_at FROM secrets WHERE name = $1",
    )
    .bind(name)
    .fetch_optional(&mut *conn)
    .await
}

pub async fn list(
    conn: &mut PgConnection,
    cursor: Option<&str>,
    limit: i64,
) -> Result<Vec<Secret>, sqlx::Error> {
    match cursor {
        Some(c) => {
            sqlx::query_as::<_, Secret>(
                "SELECT name, provider, reference, created_at, updated_at FROM secrets
                 WHERE name > $1 ORDER BY name ASC LIMIT $2",
            )
            .bind(c)
            .bind(limit)
            .fetch_all(&mut *conn)
            .await
        }
        None => {
            sqlx::query_as::<_, Secret>(
                "SELECT name, provider, reference, created_at, updated_at FROM secrets
                 ORDER BY name ASC LIMIT $1",
            )
            .bind(limit)
            .fetch_all(&mut *conn)
            .await
        }
    }
}

pub async fn list_all(conn: &mut PgConnection) -> Result<Vec<Secret>, sqlx::Error> {
    sqlx::query_as::<_, Secret>(
        "SELECT name, provider, reference, created_at, updated_at FROM secrets ORDER BY name ASC",
    )
    .fetch_all(&mut *conn)
    .await
}

pub async fn update(
    conn: &mut PgConnection,
    name: &str,
    provider: Option<&str>,
    reference: Option<&str>,
) -> Result<Option<Secret>, sqlx::Error> {
    sqlx::query_as::<_, Secret>(
        "UPDATE secrets SET
            provider = COALESCE($2, provider),
            reference = COALESCE($3, reference),
            updated_at = now()
         WHERE name = $1
         RETURNING name, provider, reference, created_at, updated_at",
    )
    .bind(name)
    .bind(provider)
    .bind(reference)
    .fetch_optional(&mut *conn)
    .await
}

pub async fn delete(conn: &mut PgConnection, name: &str) -> Result<bool, sqlx::Error> {
    let result = sqlx::query("DELETE FROM secrets WHERE name = $1")
        .bind(name)
        .execute(&mut *conn)
        .await?;
    Ok(result.rows_affected() > 0)
}

pub async fn has_dependent_endpoints(
    conn: &mut PgConnection,
    name: &str,
) -> Result<bool, sqlx::Error> {
    let row: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM endpoints WHERE spec::TEXT LIKE '%{{secret.' || $1 || '}}%'",
    )
    .bind(name)
    .fetch_one(&mut *conn)
    .await?;
    Ok(row.0 > 0)
}
