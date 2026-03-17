use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum TriggerType {
    IMMEDIATE,
    DELAYED,
    CRON,
}

impl TriggerType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::IMMEDIATE => "IMMEDIATE",
            Self::DELAYED => "DELAYED",
            Self::CRON => "CRON",
        }
    }

    pub fn from_str_val(s: &str) -> Option<Self> {
        match s {
            "IMMEDIATE" => Some(Self::IMMEDIATE),
            "DELAYED" => Some(Self::DELAYED),
            "CRON" => Some(Self::CRON),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum JobStatus {
    ACTIVE,
    RETIRED,
}

impl JobStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::ACTIVE => "ACTIVE",
            Self::RETIRED => "RETIRED",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Job {
    pub job_id: String,
    pub crdb_region: String,
    pub endpoint: String,
    pub endpoint_type: String,
    pub trigger_type: String,
    pub status: String,
    pub version: i32,
    pub previous_version_id: Option<String>,
    pub replaced_by_id: Option<String>,
    pub idempotency_key: Option<String>,
    pub input: Option<serde_json::Value>,
    pub run_at: Option<DateTime<Utc>>,
    pub cron_expression: Option<String>,
    pub cron_timezone: Option<String>,
    pub cron_starts_at: Option<DateTime<Utc>>,
    pub cron_ends_at: Option<DateTime<Utc>>,
    pub cron_next_run_at: Option<DateTime<Utc>>,
    pub cron_last_tick_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub retired_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Deserialize)]
pub struct CreateJob {
    pub endpoint: String,
    pub trigger: String,
    pub idempotency_key: Option<String>,
    pub input: Option<serde_json::Value>,
    pub run_at: Option<DateTime<Utc>>,
    pub cron: Option<String>,
    pub timezone: Option<String>,
    pub starts_at: Option<DateTime<Utc>>,
    pub ends_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateJob {
    pub cron: Option<String>,
    pub timezone: Option<String>,
    pub input: Option<serde_json::Value>,
    pub starts_at: Option<DateTime<Utc>>,
    pub ends_at: Option<DateTime<Utc>>,
}
