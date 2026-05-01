use chrono::Utc;
use kronos_common::{
    cache::{ConfigCache, SecretCache},
    crypto, db, metrics as m, template,
};
use reqwest::Client;
use sqlx::{PgConnection, PgPool};
use std::collections::HashMap;

use crate::backoff;
use crate::dispatcher::{self, DispatchResult};

pub struct PipelineContext {
    pub pool: PgPool,
    pub http_client: Client,
    pub config_cache: ConfigCache,
    pub secret_cache: SecretCache,
    pub encryption_key: String,
}

pub async fn process_execution(
    ctx: &PipelineContext,
    conn: &mut PgConnection,
    schema_name: &str,
    execution_id: &str,
    idempotency_key: &str,
    _job_id: &str,
    endpoint_name: &str,
    endpoint_type: &str,
    input: Option<&serde_json::Value>,
    attempt_count: i64,
    max_attempts: i64,
) {
    let started_at = Utc::now();

    // 1. Load endpoint
    let endpoint = match db::endpoints::get(&mut *conn, endpoint_name).await {
        Ok(Some(ep)) => ep,
        Ok(None) => {
            tracing::error!(execution_id, "Endpoint not found: {}", endpoint_name);
            let _ = db::executions::complete_failed(&mut *conn, execution_id).await;
            log_execution(
                &mut *conn,
                execution_id,
                attempt_count,
                "ERROR",
                &format!("Endpoint not found: {}", endpoint_name),
            )
            .await;
            return;
        }
        Err(e) => {
            tracing::error!(execution_id, "Failed to load endpoint: {}", e);
            let _ = db::executions::complete_failed(&mut *conn, execution_id).await;
            return;
        }
    };

    // 2. Resolve templates
    let retry_policy = endpoint.get_retry_policy();

    let config_values = if let Some(ref config_name) = endpoint.config_ref {
        match load_config(ctx, &mut *conn, config_name).await {
            Ok(vals) => vals,
            Err(e) => {
                tracing::error!(execution_id, "Config resolution failed: {}", e);
                let _ = db::executions::complete_failed(&mut *conn, execution_id).await;
                record_attempt(
                    &mut *conn,
                    execution_id,
                    attempt_count,
                    "FAILED",
                    started_at,
                    None,
                    Some(&serde_json::json!({
                        "type": "TEMPLATE_RESOLUTION_FAILED", "message": e
                    })),
                )
                .await;
                log_execution(
                    &mut *conn,
                    execution_id,
                    attempt_count,
                    "ERROR",
                    &format!("Template resolution failed: {}", e),
                )
                .await;
                return;
            }
        }
    } else {
        HashMap::new()
    };

    let secret_values = match load_secrets(ctx, &mut *conn, &endpoint.spec).await {
        Ok(vals) => vals,
        Err(e) => {
            tracing::error!(execution_id, "Secret resolution failed: {}", e);
            let _ = db::executions::complete_failed(&mut *conn, execution_id).await;
            record_attempt(
                &mut *conn,
                execution_id,
                attempt_count,
                "FAILED",
                started_at,
                None,
                Some(&serde_json::json!({
                    "type": "TEMPLATE_RESOLUTION_FAILED", "message": e
                })),
            )
            .await;
            log_execution(
                &mut *conn,
                execution_id,
                attempt_count,
                "ERROR",
                &format!("Secret resolution failed: {}", e),
            )
            .await;
            return;
        }
    };

    let input_map: HashMap<String, serde_json::Value> = input
        .and_then(|v| v.as_object())
        .map(|obj| obj.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
        .unwrap_or_default();

    let resolved_spec =
        match template::resolve(&endpoint.spec, &input_map, &config_values, &secret_values) {
            Ok(v) => v,
            Err(e) => {
                tracing::error!(execution_id, "Template resolution failed: {}", e);
                let _ = db::executions::complete_failed(&mut *conn, execution_id).await;
                record_attempt(
                    &mut *conn,
                    execution_id,
                    attempt_count,
                    "FAILED",
                    started_at,
                    None,
                    Some(&serde_json::json!({
                        "type": "TEMPLATE_RESOLUTION_FAILED", "message": e
                    })),
                )
                .await;
                log_execution(
                    &mut *conn,
                    execution_id,
                    attempt_count,
                    "ERROR",
                    &format!("Template resolution failed: {}", e),
                )
                .await;
                return;
            }
        };

    // 3. Inject job input as body if no body/body_template in resolved spec
    let mut dispatch_spec = resolved_spec;
    if dispatch_spec.get("body").is_none() && dispatch_spec.get("body_template").is_none() {
        if let Some(input_val) = input {
            if let Some(obj) = dispatch_spec.as_object_mut() {
                obj.insert("body".to_string(), input_val.clone());
            }
        }
    }

    // 3. Dispatch
    log_execution(
        &mut *conn,
        execution_id,
        attempt_count,
        "INFO",
        &format!("Dispatching {} to {}", endpoint_type, endpoint_name),
    )
    .await;

    let result = match endpoint_type {
        "HTTP" => {
            dispatcher::http::dispatch(&ctx.http_client, &dispatch_spec, idempotency_key).await
        }
        #[cfg(feature = "kafka")]
        "KAFKA" => dispatcher::kafka::dispatch(&dispatch_spec).await,
        #[cfg(feature = "redis-stream")]
        "REDIS_STREAM" => dispatcher::redis_stream::dispatch(&dispatch_spec).await,
        _ => {
            tracing::error!(execution_id, "Unsupported endpoint type: {}", endpoint_type);
            DispatchResult::Failure {
                error: serde_json::json!({ "type": "UNSUPPORTED_TYPE", "message": format!("Unsupported: {}", endpoint_type) }),
            }
        }
    };

    let completed_at = Utc::now();
    let duration_ms = (completed_at - started_at).num_milliseconds();
    let duration_secs = duration_ms as f64 / 1000.0;

    // 4. Record attempt + finalize
    match result {
        DispatchResult::Success { output } => {
            metrics::counter!(m::EXECUTIONS_COMPLETED_TOTAL,
                "status" => "SUCCESS",
                "schema" => schema_name.to_string(),
                "endpoint" => endpoint_name.to_string(),
            )
            .increment(1);
            metrics::histogram!(m::EXECUTION_DURATION_SECONDS,
                "status" => "SUCCESS",
                "endpoint" => endpoint_name.to_string(),
                "endpoint_type" => endpoint_type.to_string(),
            )
            .record(duration_secs);

            record_attempt(
                &mut *conn,
                execution_id,
                attempt_count,
                "SUCCESS",
                started_at,
                Some(&output),
                None,
            )
            .await;
            let _ = db::executions::complete_success(&mut *conn, execution_id, &output).await;
            log_execution(
                &mut *conn,
                execution_id,
                attempt_count,
                "INFO",
                &format!("Execution succeeded in {}ms", duration_ms),
            )
            .await;
        }
        DispatchResult::Failure { error } => {
            record_attempt(
                &mut *conn,
                execution_id,
                attempt_count,
                "FAILED",
                started_at,
                None,
                Some(&error),
            )
            .await;

            if attempt_count < max_attempts {
                let backoff_ms = backoff::compute_backoff(&retry_policy, attempt_count);
                let _ = db::executions::complete_retry(&mut *conn, execution_id, backoff_ms).await;
                log_execution(
                    &mut *conn,
                    execution_id,
                    attempt_count,
                    "WARN",
                    &format!(
                        "Attempt {} failed, retrying in {}ms: {}",
                        attempt_count, backoff_ms, error
                    ),
                )
                .await;
            } else {
                metrics::counter!(m::EXECUTIONS_COMPLETED_TOTAL,
                    "status" => "FAILED",
                    "schema" => schema_name.to_string(),
                    "endpoint" => endpoint_name.to_string(),
                )
                .increment(1);
                metrics::histogram!(m::EXECUTION_DURATION_SECONDS,
                    "status" => "FAILED",
                    "endpoint" => endpoint_name.to_string(),
                    "endpoint_type" => endpoint_type.to_string(),
                )
                .record(duration_secs);

                let _ = db::executions::complete_failed(&mut *conn, execution_id).await;
                log_execution(
                    &mut *conn,
                    execution_id,
                    attempt_count,
                    "ERROR",
                    &format!(
                        "Execution failed after {} attempts: {}",
                        attempt_count, error
                    ),
                )
                .await;
            }
        }
    }
}

async fn load_config(
    ctx: &PipelineContext,
    conn: &mut sqlx::PgConnection,
    name: &str,
) -> Result<HashMap<String, serde_json::Value>, String> {
    if let Some(cached) = ctx.config_cache.get(name) {
        return flatten_json_object(&cached);
    }

    let config = db::configs::get(conn, name)
        .await
        .map_err(|e| format!("Failed to load config '{}': {}", name, e))?
        .ok_or_else(|| format!("Config '{}' not found", name))?;

    ctx.config_cache
        .set(name.to_string(), config.values_json.clone());
    flatten_json_object(&config.values_json)
}

async fn load_secrets(
    ctx: &PipelineContext,
    conn: &mut sqlx::PgConnection,
    spec: &serde_json::Value,
) -> Result<HashMap<String, String>, String> {
    let spec_str = spec.to_string();
    let mut secrets = HashMap::new();

    let mut start = 0;
    while let Some(pos) = spec_str[start..].find("{{secret.") {
        let abs_pos = start + pos + 9; // skip "{{secret."
        if let Some(end) = spec_str[abs_pos..].find("}}") {
            let secret_name = &spec_str[abs_pos..abs_pos + end];

            if !secrets.contains_key(secret_name) {
                let value = load_single_secret(ctx, conn, secret_name).await?;
                secrets.insert(secret_name.to_string(), value);
            }
            start = abs_pos + end + 2;
        } else {
            break;
        }
    }

    Ok(secrets)
}

async fn load_single_secret(
    ctx: &PipelineContext,
    conn: &mut sqlx::PgConnection,
    name: &str,
) -> Result<String, String> {
    if let Some(cached) = ctx.secret_cache.get(name) {
        return Ok(cached);
    }

    let secret = db::secrets::get(conn, name)
        .await
        .map_err(|e| format!("Failed to load secret '{}': {}", name, e))?
        .ok_or_else(|| format!("Secret '{}' not found", name))?;

    let decrypted = crypto::decrypt(&secret.encrypted_value, &ctx.encryption_key)
        .map_err(|e| format!("Failed to decrypt secret '{}': {}", name, e))?;

    ctx.secret_cache.set(name.to_string(), decrypted.clone());
    Ok(decrypted)
}

fn flatten_json_object(
    value: &serde_json::Value,
) -> Result<HashMap<String, serde_json::Value>, String> {
    let obj = value
        .as_object()
        .ok_or("Config values must be a JSON object")?;
    Ok(obj.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
}

async fn record_attempt(
    conn: &mut sqlx::PgConnection,
    execution_id: &str,
    attempt_number: i64,
    status: &str,
    started_at: chrono::DateTime<Utc>,
    output: Option<&serde_json::Value>,
    error: Option<&serde_json::Value>,
) {
    let completed_at = Utc::now();
    let duration_ms = (completed_at - started_at).num_milliseconds();
    if let Err(e) = db::attempts::insert(
        conn,
        execution_id,
        attempt_number,
        status,
        started_at,
        completed_at,
        duration_ms,
        output,
        error,
    )
    .await
    {
        tracing::error!(execution_id, "Failed to record attempt: {}", e);
    }
}

async fn log_execution(
    conn: &mut sqlx::PgConnection,
    execution_id: &str,
    attempt_number: i64,
    level: &str,
    message: &str,
) {
    if let Err(e) =
        db::execution_logs::insert(conn, execution_id, attempt_number, level, message).await
    {
        tracing::error!(execution_id, "Failed to write execution log: {}", e);
    }
}
