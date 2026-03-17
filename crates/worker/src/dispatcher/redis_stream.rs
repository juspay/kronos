use redis::AsyncCommands;
use serde_json::Value;
use super::DispatchResult;

pub async fn dispatch(spec: &Value) -> DispatchResult {
    let redis_url = spec["redis_url"].as_str().unwrap_or("redis://127.0.0.1:6379");
    let stream = spec["stream"].as_str().unwrap_or_default();
    let max_len = spec.get("max_len").and_then(|v| v.as_u64());
    let approximate = spec.get("approximate_trimming").and_then(|v| v.as_bool()).unwrap_or(true);

    let fields_template = match spec.get("fields_template").and_then(|v| v.as_object()) {
        Some(f) => f,
        None => {
            return DispatchResult::Failure {
                error: serde_json::json!({
                    "type": "STREAM_ERROR",
                    "message": "Missing fields_template in spec",
                }),
            };
        }
    };

    let client = match redis::Client::open(redis_url) {
        Ok(c) => c,
        Err(e) => {
            return DispatchResult::Failure {
                error: serde_json::json!({
                    "type": "CONNECTION_ERROR",
                    "message": format!("Failed to connect to Redis: {}", e),
                }),
            };
        }
    };

    let mut conn = match client.get_multiplexed_async_connection().await {
        Ok(c) => c,
        Err(e) => {
            return DispatchResult::Failure {
                error: serde_json::json!({
                    "type": "CONNECTION_ERROR",
                    "message": format!("Redis connection failed: {}", e),
                }),
            };
        }
    };

    // Build field pairs for XADD
    let fields: Vec<(String, String)> = fields_template
        .iter()
        .map(|(k, v)| {
            let val = match v {
                Value::String(s) => s.clone(),
                other => other.to_string(),
            };
            (k.clone(), val)
        })
        .collect();

    let field_refs: Vec<(&str, &str)> = fields.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();

    // XADD with optional MAXLEN trimming
    let result: Result<String, redis::RedisError> = if let Some(maxlen) = max_len {
        if approximate {
            redis::cmd("XADD")
                .arg(stream)
                .arg("MAXLEN")
                .arg("~")
                .arg(maxlen)
                .arg("*")
                .arg(&field_refs)
                .query_async(&mut conn)
                .await
        } else {
            redis::cmd("XADD")
                .arg(stream)
                .arg("MAXLEN")
                .arg(maxlen)
                .arg("*")
                .arg(&field_refs)
                .query_async(&mut conn)
                .await
        }
    } else {
        redis::cmd("XADD")
            .arg(stream)
            .arg("*")
            .arg(&field_refs)
            .query_async(&mut conn)
            .await
    };

    match result {
        Ok(message_id) => DispatchResult::Success {
            output: serde_json::json!({
                "message_id": message_id,
                "stream": stream,
            }),
        },
        Err(e) => {
            let error_type = if e.to_string().contains("timeout") {
                "TIMEOUT"
            } else {
                "STREAM_ERROR"
            };
            DispatchResult::Failure {
                error: serde_json::json!({
                    "type": error_type,
                    "message": e.to_string(),
                }),
            }
        }
    }
}
