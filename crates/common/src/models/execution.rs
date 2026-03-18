use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ExecutionStatus {
    PENDING,
    QUEUED,
    RUNNING,
    RETRYING,
    SUCCESS,
    FAILED,
    CANCELLED,
}

impl ExecutionStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::PENDING => "PENDING",
            Self::QUEUED => "QUEUED",
            Self::RUNNING => "RUNNING",
            Self::RETRYING => "RETRYING",
            Self::SUCCESS => "SUCCESS",
            Self::FAILED => "FAILED",
            Self::CANCELLED => "CANCELLED",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Execution {
    pub execution_id: String,
    pub job_id: String,
    pub endpoint: String,
    pub endpoint_type: String,
    pub idempotency_key: Option<String>,
    pub status: String,
    pub input: Option<serde_json::Value>,
    pub output: Option<serde_json::Value>,
    pub attempt_count: i64,
    pub max_attempts: i64,
    pub worker_id: Option<String>,
    pub run_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub duration_ms: Option<i64>,
    pub created_at: DateTime<Utc>,
}
