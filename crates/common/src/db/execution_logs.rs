use crate::{db::DbContext, models::ExecutionLog};

pub async fn insert(
    db: &mut DbContext<'_>,
    execution_id: &str,
    attempt_number: i64,
    level: &str,
    message: &str,
) -> Result<(), sqlx::Error> {
    let t = db.tbl("execution_logs");
    sqlx::query(&format!(
        "INSERT INTO {t} (execution_id, attempt_number, level, message)
         VALUES ($1, $2, $3, $4)"
    ))
    .bind(execution_id)
    .bind(attempt_number)
    .bind(level)
    .bind(message)
    .execute(&mut *db.conn)
    .await?;
    Ok(())
}

pub async fn list_for_execution(
    db: &mut DbContext<'_>,
    execution_id: &str,
) -> Result<Vec<ExecutionLog>, sqlx::Error> {
    let t = db.tbl("execution_logs");
    sqlx::query_as::<_, ExecutionLog>(&format!(
        "SELECT * FROM {t} WHERE execution_id = $1 ORDER BY logged_at ASC"
    ))
    .bind(execution_id)
    .fetch_all(&mut *db.conn)
    .await
}
