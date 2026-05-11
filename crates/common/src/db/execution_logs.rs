use crate::{db::tbl, models::ExecutionLog};
use sqlx::PgConnection;

pub async fn insert(
    conn: &mut PgConnection,
    prefix: &str,
    execution_id: &str,
    attempt_number: i64,
    level: &str,
    message: &str,
) -> Result<(), sqlx::Error> {
    let t = tbl(prefix, "execution_logs");
    sqlx::query(&format!(
        "INSERT INTO {t} (execution_id, attempt_number, level, message)
         VALUES ($1, $2, $3, $4)"
    ))
    .bind(execution_id)
    .bind(attempt_number)
    .bind(level)
    .bind(message)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

pub async fn list_for_execution(
    conn: &mut PgConnection,
    prefix: &str,
    execution_id: &str,
) -> Result<Vec<ExecutionLog>, sqlx::Error> {
    let t = tbl(prefix, "execution_logs");
    sqlx::query_as::<_, ExecutionLog>(&format!(
        "SELECT * FROM {t} WHERE execution_id = $1 ORDER BY logged_at ASC"
    ))
    .bind(execution_id)
    .fetch_all(&mut *conn)
    .await
}
