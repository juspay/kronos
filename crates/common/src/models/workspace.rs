use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Workspace {
    pub workspace_id: String,
    pub org_id: String,
    pub name: String,
    pub slug: String,
    pub schema_name: String,
    pub status: String,
    pub schema_version: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct CreateWorkspace {
    pub name: String,
    pub slug: String,
}
