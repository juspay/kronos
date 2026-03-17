use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

#[derive(Debug, Clone, FromRow)]
pub struct Secret {
    pub name: String,
    pub encrypted_value: Vec<u8>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SecretResponse {
    pub name: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl From<Secret> for SecretResponse {
    fn from(s: Secret) -> Self {
        Self {
            name: s.name,
            created_at: s.created_at,
            updated_at: s.updated_at,
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct CreateSecret {
    pub name: String,
    pub value: String,
}

#[derive(Debug, Deserialize)]
pub struct UpdateSecret {
    pub value: String,
}
