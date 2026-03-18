use crate::models::ExecutionLog;
use sqlx::PgPool;

pub async fn insert(
    pool: &PgPool,
    execution_id: &str,
    attempt_number: i64,
    level: &str,
    message: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO execution_logs (execution_id, attempt_number, level, message)
         VALUES ($1, $2, $3, $4)",
    )
    .bind(execution_id)
    .bind(attempt_number)
    .bind(level)
    .bind(message)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn list_for_execution(
    pool: &PgPool,
    execution_id: &str,
) -> Result<Vec<ExecutionLog>, sqlx::Error> {
    sqlx::query_as::<_, ExecutionLog>(
        "SELECT * FROM execution_logs WHERE execution_id = $1 ORDER BY logged_at ASC",
    )
    .bind(execution_id)
    .fetch_all(pool)
    .await
}
