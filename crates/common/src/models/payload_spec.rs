use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct PayloadSpec {
    pub name: String,
    pub schema_json: serde_json::Value,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct CreatePayloadSpec {
    pub name: String,
    pub schema: serde_json::Value,
}

#[derive(Debug, Deserialize)]
pub struct UpdatePayloadSpec {
    pub schema: serde_json::Value,
}
