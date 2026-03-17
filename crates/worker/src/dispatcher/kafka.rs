use rdkafka::config::ClientConfig;
use rdkafka::producer::{FutureProducer, FutureRecord};
use serde_json::Value;
use std::time::Duration;
use super::DispatchResult;

pub async fn dispatch(spec: &Value) -> DispatchResult {
    let bootstrap_servers = spec["bootstrap_servers"].as_str().unwrap_or_default();
    let topic = spec["topic"].as_str().unwrap_or_default();
    let timeout_ms = spec["timeout_ms"].as_u64().unwrap_or(10000);
    let acks = spec["acks"].as_str().unwrap_or("all");

    let key = spec.get("key_template")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let value = spec.get("value_template")
        .map(|v| v.to_string())
        .unwrap_or_else(|| "{}".to_string());

    let producer: FutureProducer = match ClientConfig::new()
        .set("bootstrap.servers", bootstrap_servers)
        .set("message.timeout.ms", &timeout_ms.to_string())
        .set("acks", acks)
        .create()
    {
        Ok(p) => p,
        Err(e) => {
            return DispatchResult::Failure {
                error: serde_json::json!({
                    "type": "BROKER_ERROR",
                    "message": format!("Failed to create producer: {}", e),
                }),
            };
        }
    };

    let mut record = FutureRecord::to(topic).payload(&value);
    if let Some(ref k) = key {
        record = record.key(k);
    }

    // Set headers if present
    if let Some(headers_obj) = spec.get("headers").and_then(|h| h.as_object()) {
        let mut headers = rdkafka::message::OwnedHeaders::new();
        for (k, v) in headers_obj {
            if let Some(val) = v.as_str() {
                headers = headers.insert(rdkafka::message::Header { key: k, value: Some(val) });
            }
        }
        record = record.headers(headers);
    }

    match producer.send(record, Duration::from_millis(timeout_ms)).await {
        Ok((partition, offset)) => DispatchResult::Success {
            output: serde_json::json!({
                "partition": partition,
                "offset": offset,
            }),
        },
        Err((e, _)) => DispatchResult::Failure {
            error: serde_json::json!({
                "type": if e.to_string().contains("timeout") { "TIMEOUT" } else { "BROKER_ERROR" },
                "message": e.to_string(),
            }),
        },
    }
}
