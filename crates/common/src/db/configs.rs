use crate::{db::DbContext, models::Config};

pub async fn create(
    db: &mut DbContext<'_>,
    name: &str,
    values: &serde_json::Value,
) -> Result<Config, sqlx::Error> {
    let t = db.tbl("configs");
    sqlx::query_as::<_, Config>(&format!(
        "INSERT INTO {t} (name, values_json) VALUES ($1, $2)
         RETURNING name, values_json, created_at, updated_at"
    ))
    .bind(name)
    .bind(values)
    .fetch_one(&mut *db.conn)
    .await
}

pub async fn get(
    db: &mut DbContext<'_>,
    name: &str,
) -> Result<Option<Config>, sqlx::Error> {
    let t = db.tbl("configs");
    sqlx::query_as::<_, Config>(&format!(
        "SELECT name, values_json, created_at, updated_at FROM {t} WHERE name = $1"
    ))
    .bind(name)
    .fetch_optional(&mut *db.conn)
    .await
}

pub async fn list(
    db: &mut DbContext<'_>,
    cursor: Option<&str>,
    limit: i64,
) -> Result<Vec<Config>, sqlx::Error> {
    let t = db.tbl("configs");
    match cursor {
        Some(c) => {
            sqlx::query_as::<_, Config>(&format!(
                "SELECT name, values_json, created_at, updated_at FROM {t}
                 WHERE name > $1 ORDER BY name ASC LIMIT $2"
            ))
            .bind(c)
            .bind(limit)
            .fetch_all(&mut *db.conn)
            .await
        }
        None => {
            sqlx::query_as::<_, Config>(&format!(
                "SELECT name, values_json, created_at, updated_at FROM {t}
                 ORDER BY name ASC LIMIT $1"
            ))
            .bind(limit)
            .fetch_all(&mut *db.conn)
            .await
        }
    }
}

pub async fn update(
    db: &mut DbContext<'_>,
    name: &str,
    values: &serde_json::Value,
) -> Result<Option<Config>, sqlx::Error> {
    let t = db.tbl("configs");
    sqlx::query_as::<_, Config>(&format!(
        "UPDATE {t} SET values_json = $2, updated_at = now()
         WHERE name = $1
         RETURNING name, values_json, created_at, updated_at"
    ))
    .bind(name)
    .bind(values)
    .fetch_optional(&mut *db.conn)
    .await
}

pub async fn delete(
    db: &mut DbContext<'_>,
    name: &str,
) -> Result<bool, sqlx::Error> {
    let t = db.tbl("configs");
    let result = sqlx::query(&format!("DELETE FROM {t} WHERE name = $1"))
        .bind(name)
        .execute(&mut *db.conn)
        .await?;
    Ok(result.rows_affected() > 0)
}

pub async fn has_dependent_endpoints(
    db: &mut DbContext<'_>,
    name: &str,
) -> Result<bool, sqlx::Error> {
    let te = db.tbl("endpoints");
    let row: (i64,) =
        sqlx::query_as(&format!("SELECT COUNT(*) FROM {te} WHERE config_ref = $1"))
            .bind(name)
            .fetch_one(&mut *db.conn)
            .await?;
    Ok(row.0 > 0)
}
