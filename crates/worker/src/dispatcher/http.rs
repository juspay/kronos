use reqwest::Client;
use serde_json::Value;
use std::time::Duration;
use super::DispatchResult;

pub async fn dispatch(client: &Client, spec: &Value) -> DispatchResult {
    let url = spec["url"].as_str().unwrap_or_default();
    let method = spec["method"].as_str().unwrap_or("POST");
    let timeout_ms = spec["timeout_ms"].as_u64().unwrap_or(5000);
    let expected_statuses: Vec<u16> = spec["expected_status_codes"]
        .as_array()
        .map(|arr| arr.iter().filter_map(|v| v.as_u64().map(|n| n as u16)).collect())
        .unwrap_or_else(|| vec![200, 201, 202, 204]);

    let mut req = match method.to_uppercase().as_str() {
        "GET" => client.get(url),
        "POST" => client.post(url),
        "PUT" => client.put(url),
        "PATCH" => client.patch(url),
        "DELETE" => client.delete(url),
        _ => client.post(url),
    };

    req = req.timeout(Duration::from_millis(timeout_ms));

    // Set headers
    if let Some(headers) = spec["headers"].as_object() {
        for (k, v) in headers {
            if let Some(val) = v.as_str() {
                req = req.header(k.as_str(), val);
            }
        }
    }

    // Set body
    if let Some(body) = spec.get("body_template") {
        if !body.is_null() {
            req = req.json(body);
        }
    }

    match req.send().await {
        Ok(response) => {
            let status = response.status().as_u16();
            let body = response.text().await.unwrap_or_default();

            if expected_statuses.contains(&status) {
                DispatchResult::Success {
                    output: serde_json::json!({
                        "status_code": status,
                        "body": body,
                    }),
                }
            } else {
                DispatchResult::Failure {
                    error: serde_json::json!({
                        "type": "HTTP_ERROR",
                        "status_code": status,
                        "message": format!("Unexpected status code: {}", status),
                    }),
                }
            }
        }
        Err(e) => {
            let error_type = if e.is_timeout() {
                "TIMEOUT"
            } else if e.is_connect() {
                "CONNECTION_ERROR"
            } else {
                "HTTP_ERROR"
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
