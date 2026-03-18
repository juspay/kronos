use serde::{Deserialize, Serialize};

// -- Wrapper types for API responses --

#[derive(Debug, Clone, Deserialize)]
pub struct DataResponse<T> {
    pub data: T,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PaginatedResponse<T> {
    pub data: Vec<T>,
    pub cursor: Option<String>,
}

// -- Organization --

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Organization {
    pub org_id: String,
    pub name: String,
    pub slug: String,
    pub status: String,
    pub created_at: String,
    #[serde(default)]
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CreateOrganization {
    pub name: String,
    pub slug: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct UpdateOrganization {
    pub name: String,
}

// -- Workspace --

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Workspace {
    pub workspace_id: String,
    pub org_id: String,
    pub name: String,
    pub slug: String,
    pub schema_name: String,
    pub status: String,
    pub schema_version: i64,
    pub created_at: String,
    #[serde(default)]
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CreateWorkspace {
    pub name: String,
    pub slug: String,
}

// -- Job --

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Job {
    pub job_id: String,
    pub endpoint: String,
    pub endpoint_type: String,
    pub trigger: String,
    pub status: String,
    pub version: i64,
    #[serde(default)]
    pub idempotency_key: Option<String>,
    #[serde(default)]
    pub input: Option<serde_json::Value>,
    #[serde(default)]
    pub run_at: Option<String>,
    #[serde(default)]
    pub cron: Option<String>,
    #[serde(default)]
    pub timezone: Option<String>,
    #[serde(default)]
    pub starts_at: Option<String>,
    #[serde(default)]
    pub ends_at: Option<String>,
    #[serde(default)]
    pub next_run_at: Option<String>,
    pub created_at: String,
}

// -- Endpoint --

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Endpoint {
    pub name: String,
    #[serde(rename = "type")]
    pub endpoint_type: String,
    #[serde(default, alias = "payload_spec_ref")]
    pub payload_spec: Option<String>,
    #[serde(default, alias = "config_ref")]
    pub config: Option<String>,
    pub spec: serde_json::Value,
    #[serde(default)]
    pub retry_policy: Option<serde_json::Value>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct CreateEndpoint {
    pub name: String,
    #[serde(rename = "type")]
    pub endpoint_type: String,
    pub spec: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payload_spec: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retry_policy: Option<serde_json::Value>,
}

// -- Execution --

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Execution {
    pub execution_id: String,
    pub job_id: String,
    #[serde(default)]
    pub endpoint: Option<String>,
    #[serde(default)]
    pub endpoint_type: Option<String>,
    pub status: String,
    #[serde(default)]
    pub attempt_count: Option<i64>,
    #[serde(default)]
    pub max_attempts: Option<i64>,
    #[serde(default)]
    pub input: Option<serde_json::Value>,
    #[serde(default)]
    pub output: Option<serde_json::Value>,
    #[serde(default)]
    pub worker_id: Option<String>,
    #[serde(default)]
    pub run_at: Option<String>,
    #[serde(default)]
    pub started_at: Option<String>,
    #[serde(default)]
    pub completed_at: Option<String>,
    #[serde(default)]
    pub duration_ms: Option<i64>,
    pub created_at: String,
}

// -- Config --

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Config {
    pub name: String,
    pub values: serde_json::Value,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct CreateConfig {
    pub name: String,
    pub values: serde_json::Value,
}

#[derive(Debug, Clone, Serialize)]
pub struct UpdateConfig {
    pub values: serde_json::Value,
}

// -- Payload Spec --

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PayloadSpec {
    pub name: String,
    pub schema: serde_json::Value,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct CreatePayloadSpec {
    pub name: String,
    pub schema: serde_json::Value,
}

#[derive(Debug, Clone, Serialize)]
pub struct UpdatePayloadSpec {
    pub schema: serde_json::Value,
}

// -- Secret --

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Secret {
    pub name: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct CreateSecret {
    pub name: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct UpdateSecret {
    pub value: String,
}

// -- Attempt --

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Attempt {
    pub attempt_id: String,
    pub attempt_number: i64,
    pub status: String,
    #[serde(default)]
    pub started_at: Option<String>,
    #[serde(default)]
    pub completed_at: Option<String>,
    #[serde(default)]
    pub duration_ms: Option<i64>,
    #[serde(default)]
    pub output: Option<serde_json::Value>,
    #[serde(default)]
    pub error: Option<String>,
}

// -- Execution Log --

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ExecutionLog {
    pub log_id: String,
    #[serde(default)]
    pub attempt_number: Option<i64>,
    pub level: String,
    pub message: String,
    pub logged_at: String,
}

// -- Job Status --

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct JobStatus {
    pub job_id: String,
    pub endpoint: String,
    pub endpoint_type: String,
    pub trigger: String,
    pub health: String,
    pub version: i64,
    #[serde(default)]
    pub last_execution: Option<serde_json::Value>,
    #[serde(default)]
    pub active_executions: Option<serde_json::Value>,
    #[serde(default)]
    pub cron: Option<serde_json::Value>,
}
