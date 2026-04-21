use std::fmt::Display;
use std::str::FromStr;

/// Read an environment variable and parse it into the requested type.
/// Returns an error message including the variable name on failure.
pub fn get_from_env_unsafe<F>(name: &str) -> Result<F, String>
where
    F: FromStr,
    <F as FromStr>::Err: std::fmt::Debug,
{
    std::env::var(name)
        .map_err(|e| format!("{name} env not found: {e}"))
        .and_then(|val| {
            val.parse()
                .map_err(|e| format!("Failed to parse {name}: {e:?}"))
        })
}

/// Read an environment variable and parse it, returning `default` when the
/// variable is absent or cannot be parsed.
pub fn get_from_env_or_default<F>(name: &str, default: F) -> F
where
    F: FromStr + Display,
    <F as FromStr>::Err: std::fmt::Debug,
{
    match std::env::var(name) {
        Ok(val) => val.parse().unwrap_or_else(|e| {
            tracing::warn!("{name} failed to parse ({e:?}), using default: {default}");
            default
        }),
        Err(_) => default,
    }
}
