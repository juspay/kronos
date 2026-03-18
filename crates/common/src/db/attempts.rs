use crate::models::Attempt;
use chrono::{DateTime, Utc};
use sqlx::PgConnection;

pub async fn insert(
    conn: &mut PgConnection,
    execution_id: &str,
    attempt_number: i64,
    status: &str,
    started_at: DateTime<Utc>,
    completed_at: DateTime<Utc>,
    duration_ms: i64,
    output: Option<&serde_json::Value>,
    error: Option<&serde_json::Value>,
) -> Result<Attempt, sqlx::Error> {
    sqlx::query_as::<_, Attempt>(
        "INSERT INTO attempts (execution_id, attempt_number, status, started_at, completed_at, duration_ms, output, error)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
         RETURNING *"
    )
    .bind(execution_id)
    .bind(attempt_number)
    .bind(status)
    .bind(started_at)
    .bind(completed_at)
    .bind(duration_ms)
    .bind(output)
    .bind(error)
    .fetch_one(&mut *conn)
    .await
}

pub async fn list_for_execution(
    conn: &mut PgConnection,
    execution_id: &str,
) -> Result<Vec<Attempt>, sqlx::Error> {
    sqlx::query_as::<_, Attempt>(
        "SELECT * FROM attempts WHERE execution_id = $1 ORDER BY attempt_number ASC",
    )
    .bind(execution_id)
    .fetch_all(&mut *conn)
    .await
}
