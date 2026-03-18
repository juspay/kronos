pub mod http;
#[cfg(feature = "kafka")]
pub mod kafka;
#[cfg(feature = "redis-stream")]
pub mod redis_stream;

use serde_json::Value;

pub enum DispatchResult {
    Success { output: Value },
    Failure { error: Value },
}
