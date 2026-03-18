use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum AttemptStatus {
    SUCCESS,
    FAILED,
}

impl AttemptStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::SUCCESS => "SUCCESS",
            Self::FAILED => "FAILED",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Attempt {
    pub attempt_id: String,
    pub crdb_region: String,
    pub execution_id: String,
    pub attempt_number: i64,
    pub status: String,
    pub started_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
    pub duration_ms: Option<i64>,
    pub output: Option<serde_json::Value>,
    pub error: Option<serde_json::Value>,
    pub created_at: DateTime<Utc>,
}
