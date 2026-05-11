use crate::{db::tbl, models::Config};
use sqlx::PgConnection;

pub async fn create(
    conn: &mut PgConnection,
    prefix: &str,
    name: &str,
    values: &serde_json::Value,
) -> Result<Config, sqlx::Error> {
    let t = tbl(prefix, "configs");
    sqlx::query_as::<_, Config>(&format!(
        "INSERT INTO {t} (name, values_json) VALUES ($1, $2)
         RETURNING name, values_json, created_at, updated_at"
    ))
    .bind(name)
    .bind(values)
    .fetch_one(&mut *conn)
    .await
}

pub async fn get(
    conn: &mut PgConnection,
    prefix: &str,
    name: &str,
) -> Result<Option<Config>, sqlx::Error> {
    let t = tbl(prefix, "configs");
    sqlx::query_as::<_, Config>(&format!(
        "SELECT name, values_json, created_at, updated_at FROM {t} WHERE name = $1"
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
) -> Result<Vec<Config>, sqlx::Error> {
    let t = tbl(prefix, "configs");
    match cursor {
        Some(c) => {
            sqlx::query_as::<_, Config>(&format!(
                "SELECT name, values_json, created_at, updated_at FROM {t}
                 WHERE name > $1 ORDER BY name ASC LIMIT $2"
            ))
            .bind(c)
            .bind(limit)
            .fetch_all(&mut *conn)
            .await
        }
        None => {
            sqlx::query_as::<_, Config>(&format!(
                "SELECT name, values_json, created_at, updated_at FROM {t}
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
    values: &serde_json::Value,
) -> Result<Option<Config>, sqlx::Error> {
    let t = tbl(prefix, "configs");
    sqlx::query_as::<_, Config>(&format!(
        "UPDATE {t} SET values_json = $2, updated_at = now()
         WHERE name = $1
         RETURNING name, values_json, created_at, updated_at"
    ))
    .bind(name)
    .bind(values)
    .fetch_optional(&mut *conn)
    .await
}

pub async fn delete(
    conn: &mut PgConnection,
    prefix: &str,
    name: &str,
) -> Result<bool, sqlx::Error> {
    let t = tbl(prefix, "configs");
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
    let row: (i64,) =
        sqlx::query_as(&format!("SELECT COUNT(*) FROM {te} WHERE config_ref = $1"))
            .bind(name)
            .fetch_one(&mut *conn)
            .await?;
    Ok(row.0 > 0)
}
