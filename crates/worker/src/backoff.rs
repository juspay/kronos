use kronos_common::models::endpoint::RetryPolicy;
use rand::Rng;

pub fn compute_backoff(policy: &RetryPolicy, attempt: i64) -> i64 {
    let delay = match policy.backoff.as_str() {
        "fixed" => policy.initial_delay_ms,
        "linear" => policy.initial_delay_ms * attempt,
        "exponential" | _ => policy.initial_delay_ms * 2_i64.pow((attempt - 1).max(0) as u32),
    };

    // Add ±25% jitter
    let jitter_range = delay / 4;
    let jitter = if jitter_range > 0 {
        rand::thread_rng().gen_range(-jitter_range..=jitter_range)
    } else {
        0
    };

    (delay + jitter).clamp(0, policy.max_delay_ms)
}
