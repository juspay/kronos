use crate::{db::tbl, models::secret::Secret};
use sqlx::PgConnection;

pub async fn create(
    conn: &mut PgConnection,
    prefix: &str,
    name: &str,
    encrypted_value: &[u8],
) -> Result<Secret, sqlx::Error> {
    let t = tbl(prefix, "secrets");
    sqlx::query_as::<_, Secret>(&format!(
        "INSERT INTO {t} (name, encrypted_value) VALUES ($1, $2)
         RETURNING name, encrypted_value, created_at, updated_at"
    ))
    .bind(name)
    .bind(encrypted_value)
    .fetch_one(&mut *conn)
    .await
}

pub async fn get(
    conn: &mut PgConnection,
    prefix: &str,
    name: &str,
) -> Result<Option<Secret>, sqlx::Error> {
    let t = tbl(prefix, "secrets");
    sqlx::query_as::<_, Secret>(&format!(
        "SELECT name, encrypted_value, created_at, updated_at FROM {t} WHERE name = $1"
    ))
    .bind(name)
    .fetch_optional(&mut *conn)
    .await
}

pub async fn list(
    conn: &mut PgConnection,
    prefix: &str,
    cursor: Option<&str>,
    limit: i64,
) -> Result<Vec<Secret>, sqlx::Error> {
    let t = tbl(prefix, "secrets");
    match cursor {
        Some(c) => {
            sqlx::query_as::<_, Secret>(&format!(
                "SELECT name, encrypted_value, created_at, updated_at FROM {t}
                 WHERE name > $1 ORDER BY name ASC LIMIT $2"
            ))
            .bind(c)
            .bind(limit)
            .fetch_all(&mut *conn)
            .await
        }
        None => {
            sqlx::query_as::<_, Secret>(&format!(
                "SELECT name, encrypted_value, created_at, updated_at FROM {t}
                 ORDER BY name ASC LIMIT $1"
            ))
            .bind(limit)
            .fetch_all(&mut *conn)
            .await
        }
    }
}

pub async fn update(
    conn: &mut PgConnection,
    prefix: &str,
    name: &str,
    encrypted_value: &[u8],
) -> Result<Option<Secret>, sqlx::Error> {
    let t = tbl(prefix, "secrets");
    sqlx::query_as::<_, Secret>(&format!(
        "UPDATE {t} SET encrypted_value = $2, updated_at = now()
         WHERE name = $1
         RETURNING name, encrypted_value, created_at, updated_at"
    ))
    .bind(name)
    .bind(encrypted_value)
    .fetch_optional(&mut *conn)
    .await
}

pub async fn delete(
    conn: &mut PgConnection,
    prefix: &str,
    name: &str,
) -> Result<bool, sqlx::Error> {
    let t = tbl(prefix, "secrets");
    let result = sqlx::query(&format!("DELETE FROM {t} WHERE name = $1"))
        .bind(name)
        .execute(&mut *conn)
        .await?;
    Ok(result.rows_affected() > 0)
}

pub async fn has_dependent_endpoints(
    conn: &mut PgConnection,
    prefix: &str,
    name: &str,
) -> Result<bool, sqlx::Error> {
    let te = tbl(prefix, "endpoints");
    let row: (i64,) = sqlx::query_as(&format!(
        "SELECT COUNT(*) FROM {te} WHERE spec::TEXT LIKE '%{{{{secret.' || $1 || '}}}}%'"
    ))
    .bind(name)
    .fetch_one(&mut *conn)
    .await?;
    Ok(row.0 > 0)
}
