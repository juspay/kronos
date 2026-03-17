use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Config {
    pub name: String,
    pub values_json: serde_json::Value,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct CreateConfig {
    pub name: String,
    pub values: serde_json::Value,
}

#[derive(Debug, Deserialize)]
pub struct UpdateConfig {
    pub values: serde_json::Value,
}
