use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use std::fmt;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[allow(non_camel_case_types)]
pub enum EndpointType {
    HTTP,
    KAFKA,
    REDIS_STREAM,
}

impl fmt::Display for EndpointType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::HTTP => write!(f, "HTTP"),
            Self::KAFKA => write!(f, "KAFKA"),
            Self::REDIS_STREAM => write!(f, "REDIS_STREAM"),
        }
    }
}

impl EndpointType {
    pub fn from_str_val(s: &str) -> Option<Self> {
        match s {
            "HTTP" => Some(Self::HTTP),
            "KAFKA" => Some(Self::KAFKA),
            "REDIS_STREAM" => Some(Self::REDIS_STREAM),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetryPolicy {
    #[serde(default = "default_max_attempts")]
    pub max_attempts: i64,
    #[serde(default = "default_backoff")]
    pub backoff: String,
    #[serde(default = "default_initial_delay")]
    pub initial_delay_ms: i64,
    #[serde(default = "default_max_delay")]
    pub max_delay_ms: i64,
}

fn default_max_attempts() -> i64 {
    1
}
fn default_backoff() -> String {
    "exponential".into()
}
fn default_initial_delay() -> i64 {
    1000
}
fn default_max_delay() -> i64 {
    60000
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_attempts: default_max_attempts(),
            backoff: default_backoff(),
            initial_delay_ms: default_initial_delay(),
            max_delay_ms: default_max_delay(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Endpoint {
    pub name: String,
    pub endpoint_type: String,
    pub payload_spec_ref: Option<String>,
    pub config_ref: Option<String>,
    pub spec: serde_json::Value,
    pub retry_policy: Option<serde_json::Value>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Endpoint {
    pub fn get_retry_policy(&self) -> RetryPolicy {
        self.retry_policy
            .as_ref()
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default()
    }
}

#[derive(Debug, Deserialize)]
pub struct CreateEndpoint {
    pub name: String,
    #[serde(rename = "type")]
    pub endpoint_type: String,
    pub payload_spec: Option<String>,
    pub config: Option<String>,
    pub spec: serde_json::Value,
    pub retry_policy: Option<RetryPolicy>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateEndpoint {
    pub spec: Option<serde_json::Value>,
    pub config: Option<String>,
    pub payload_spec: Option<String>,
    pub retry_policy: Option<RetryPolicy>,
}
