use crate::{
    db::{tbl, DbContext},
    models::Endpoint,
};

pub async fn create(
    db: &mut DbContext<'_>,
    name: &str,
    endpoint_type: &str,
    payload_spec_ref: Option<&str>,
    config_ref: Option<&str>,
    spec: &serde_json::Value,
    retry_policy: Option<&serde_json::Value>,
) -> Result<Endpoint, sqlx::Error> {
    let t = tbl(db.prefix, "endpoints");
    sqlx::query_as::<_, Endpoint>(&format!(
        "INSERT INTO {t} (name, endpoint_type, payload_spec_ref, config_ref, spec, retry_policy)
         VALUES ($1, $2, $3, $4, $5, $6)
         RETURNING name, endpoint_type, payload_spec_ref, config_ref, spec, retry_policy, created_at, updated_at"
    ))
    .bind(name)
    .bind(endpoint_type)
    .bind(payload_spec_ref)
    .bind(config_ref)
    .bind(spec)
    .bind(retry_policy)
    .fetch_one(&mut *db.conn)
    .await
}

pub async fn get(db: &mut DbContext<'_>, name: &str) -> Result<Option<Endpoint>, sqlx::Error> {
    let t = tbl(db.prefix, "endpoints");
    sqlx::query_as::<_, Endpoint>(&format!(
        "SELECT name, endpoint_type, payload_spec_ref, config_ref, spec, retry_policy, created_at, updated_at
         FROM {t} WHERE name = $1"
    ))
    .bind(name)
    .fetch_optional(&mut *db.conn)
    .await
}

pub async fn list(
    db: &mut DbContext<'_>,
    cursor: Option<&str>,
    limit: i64,
) -> Result<Vec<Endpoint>, sqlx::Error> {
    let t = tbl(db.prefix, "endpoints");
    match cursor {
        Some(c) => {
            sqlx::query_as::<_, Endpoint>(&format!(
                "SELECT name, endpoint_type, payload_spec_ref, config_ref, spec, retry_policy, created_at, updated_at
                 FROM {t} WHERE name > $1 ORDER BY name ASC LIMIT $2"
            ))
            .bind(c)
            .bind(limit)
            .fetch_all(&mut *db.conn)
            .await
        }
        None => {
            sqlx::query_as::<_, Endpoint>(&format!(
                "SELECT name, endpoint_type, payload_spec_ref, config_ref, spec, retry_policy, created_at, updated_at
                 FROM {t} ORDER BY name ASC LIMIT $1"
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
    spec: Option<&serde_json::Value>,
    config_ref: Option<&str>,
    payload_spec_ref: Option<&str>,
    retry_policy: Option<&serde_json::Value>,
) -> Result<Option<Endpoint>, sqlx::Error> {
    let t = tbl(db.prefix, "endpoints");
    sqlx::query_as::<_, Endpoint>(&format!(
        "UPDATE {t} SET
            spec = COALESCE($2, spec),
            config_ref = COALESCE($3, config_ref),
            payload_spec_ref = COALESCE($4, payload_spec_ref),
            retry_policy = COALESCE($5, retry_policy),
            updated_at = now()
         WHERE name = $1
         RETURNING name, endpoint_type, payload_spec_ref, config_ref, spec, retry_policy, created_at, updated_at"
    ))
    .bind(name)
    .bind(spec)
    .bind(config_ref)
    .bind(payload_spec_ref)
    .bind(retry_policy)
    .fetch_optional(&mut *db.conn)
    .await
}

pub async fn delete(db: &mut DbContext<'_>, name: &str) -> Result<bool, sqlx::Error> {
    let t = tbl(db.prefix, "endpoints");
    let result = sqlx::query(&format!("DELETE FROM {t} WHERE name = $1"))
        .bind(name)
        .execute(&mut *db.conn)
        .await?;
    Ok(result.rows_affected() > 0)
}

pub async fn has_active_jobs(db: &mut DbContext<'_>, name: &str) -> Result<bool, sqlx::Error> {
    let tj = tbl(db.prefix, "jobs");
    let row: (i64,) = sqlx::query_as(&format!(
        "SELECT COUNT(*) FROM {tj} WHERE endpoint = $1 AND status = 'ACTIVE'"
    ))
    .bind(name)
    .fetch_one(&mut *db.conn)
    .await?;
    Ok(row.0 > 0)
}
