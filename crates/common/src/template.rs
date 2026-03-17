use std::collections::HashMap;

pub fn resolve(
    value: &serde_json::Value,
    input: &HashMap<String, serde_json::Value>,
    config: &HashMap<String, serde_json::Value>,
    secrets: &HashMap<String, String>,
) -> Result<serde_json::Value, String> {
    match value {
        serde_json::Value::String(s) => resolve_string(s, input, config, secrets),
        serde_json::Value::Object(map) => {
            let mut result = serde_json::Map::new();
            for (k, v) in map {
                let resolved_key = match resolve_string(k, input, config, secrets)? {
                    serde_json::Value::String(s) => s,
                    other => other.to_string(),
                };
                result.insert(resolved_key, resolve(v, input, config, secrets)?);
            }
            Ok(serde_json::Value::Object(result))
        }
        serde_json::Value::Array(arr) => {
            let resolved: Result<Vec<_>, _> = arr
                .iter()
                .map(|v| resolve(v, input, config, secrets))
                .collect();
            Ok(serde_json::Value::Array(resolved?))
        }
        other => Ok(other.clone()),
    }
}

fn resolve_string(
    s: &str,
    input: &HashMap<String, serde_json::Value>,
    config: &HashMap<String, serde_json::Value>,
    secrets: &HashMap<String, String>,
) -> Result<serde_json::Value, String> {
    // Check if the entire string is a single template variable
    if let Some(var) = is_single_template(s) {
        return resolve_variable(var, input, config, secrets);
    }

    // Otherwise do string interpolation
    let mut result = s.to_string();
    let mut start = 0;
    while let Some(open) = result[start..].find("{{") {
        let open = start + open;
        if let Some(close) = result[open..].find("}}") {
            let close = open + close + 2;
            let var = result[open + 2..close - 2].trim();
            let resolved = resolve_variable(var, input, config, secrets)?;
            let replacement = match &resolved {
                serde_json::Value::String(s) => s.clone(),
                other => other.to_string(),
            };
            result.replace_range(open..close, &replacement);
            start = open + replacement.len();
        } else {
            break;
        }
    }
    Ok(serde_json::Value::String(result))
}

fn is_single_template(s: &str) -> Option<&str> {
    let trimmed = s.trim();
    if trimmed.starts_with("{{") && trimmed.ends_with("}}") {
        let inner = trimmed[2..trimmed.len() - 2].trim();
        // Make sure there's no other {{ in the inner part
        if !inner.contains("{{") {
            return Some(inner);
        }
    }
    None
}

fn resolve_variable(
    var: &str,
    input: &HashMap<String, serde_json::Value>,
    config: &HashMap<String, serde_json::Value>,
    secrets: &HashMap<String, String>,
) -> Result<serde_json::Value, String> {
    if let Some(key) = var.strip_prefix("input.") {
        input
            .get(key)
            .cloned()
            .ok_or_else(|| format!("Unresolved template variable: {}", var))
    } else if let Some(key) = var.strip_prefix("config.") {
        config
            .get(key)
            .cloned()
            .ok_or_else(|| format!("Unresolved template variable: {}", var))
    } else if let Some(key) = var.strip_prefix("secret.") {
        secrets
            .get(key)
            .map(|v| serde_json::Value::String(v.clone()))
            .ok_or_else(|| format!("Unresolved template variable: {}", var))
    } else {
        Err(format!("Unknown template namespace in: {}", var))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_resolution() {
        let mut input = HashMap::new();
        input.insert("user_id".into(), serde_json::json!("u_abc"));

        let mut config = HashMap::new();
        config.insert("api_base_url".into(), serde_json::json!("https://api.example.com"));

        let mut secrets = HashMap::new();
        secrets.insert("api_key".into(), "sk-123".into());

        let template = serde_json::json!({
            "url": "{{config.api_base_url}}/users/{{input.user_id}}",
            "headers": {
                "Authorization": "Bearer {{secret.api_key}}"
            },
            "body": {
                "user_id": "{{input.user_id}}"
            }
        });

        let result = resolve(&template, &input, &config, &secrets).unwrap();
        assert_eq!(
            result["url"],
            serde_json::json!("https://api.example.com/users/u_abc")
        );
        assert_eq!(
            result["headers"]["Authorization"],
            serde_json::json!("Bearer sk-123")
        );
        assert_eq!(result["body"]["user_id"], serde_json::json!("u_abc"));
    }

    #[test]
    fn test_missing_variable() {
        let input = HashMap::new();
        let config = HashMap::new();
        let secrets = HashMap::new();

        let template = serde_json::json!("{{input.missing}}");
        assert!(resolve(&template, &input, &config, &secrets).is_err());
    }
}
