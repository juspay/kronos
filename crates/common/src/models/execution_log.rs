use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct ExecutionLog {
    pub log_id: String,
    pub crdb_region: String,
    pub execution_id: String,
    pub attempt_number: i64,
    pub level: String,
    pub message: String,
    pub logged_at: DateTime<Utc>,
}
