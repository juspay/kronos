use crate::{
    db::{tbl, DbContext},
    models::PayloadSpec,
};

pub async fn create(
    db: &mut DbContext<'_>,
    name: &str,
    schema: &serde_json::Value,
) -> Result<PayloadSpec, sqlx::Error> {
    let t = tbl(db.prefix, "payload_specs");
    sqlx::query_as::<_, PayloadSpec>(&format!(
        "INSERT INTO {t} (name, schema_json) VALUES ($1, $2)
         RETURNING name, schema_json, created_at, updated_at"
    ))
    .bind(name)
    .bind(schema)
    .fetch_one(&mut *db.conn)
    .await
}

pub async fn get(db: &mut DbContext<'_>, name: &str) -> Result<Option<PayloadSpec>, sqlx::Error> {
    let t = tbl(db.prefix, "payload_specs");
    sqlx::query_as::<_, PayloadSpec>(&format!(
        "SELECT name, schema_json, created_at, updated_at FROM {t} WHERE name = $1"
    ))
    .bind(name)
    .fetch_optional(&mut *db.conn)
    .await
}

pub async fn list(
    db: &mut DbContext<'_>,
    cursor: Option<&str>,
    limit: i64,
) -> Result<Vec<PayloadSpec>, sqlx::Error> {
    let t = tbl(db.prefix, "payload_specs");
    match cursor {
        Some(c) => {
            sqlx::query_as::<_, PayloadSpec>(&format!(
                "SELECT name, schema_json, created_at, updated_at FROM {t}
                 WHERE name > $1 ORDER BY name ASC LIMIT $2"
            ))
            .bind(c)
            .bind(limit)
            .fetch_all(&mut *db.conn)
            .await
        }
        None => {
            sqlx::query_as::<_, PayloadSpec>(&format!(
                "SELECT name, schema_json, created_at, updated_at FROM {t}
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
    schema: &serde_json::Value,
) -> Result<Option<PayloadSpec>, sqlx::Error> {
    let t = tbl(db.prefix, "payload_specs");
    sqlx::query_as::<_, PayloadSpec>(&format!(
        "UPDATE {t} SET schema_json = $2, updated_at = now()
         WHERE name = $1
         RETURNING name, schema_json, created_at, updated_at"
    ))
    .bind(name)
    .bind(schema)
    .fetch_optional(&mut *db.conn)
    .await
}

pub async fn delete(db: &mut DbContext<'_>, name: &str) -> Result<bool, sqlx::Error> {
    let t = tbl(db.prefix, "payload_specs");
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
    let te = tbl(db.prefix, "endpoints");
    let row: (i64,) = sqlx::query_as(&format!(
        "SELECT COUNT(*) FROM {te} WHERE payload_spec_ref = $1"
    ))
    .bind(name)
    .fetch_one(&mut *db.conn)
    .await?;
    Ok(row.0 > 0)
}
