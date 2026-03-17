use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct PaginationParams {
    pub cursor: Option<String>,
    #[serde(default = "default_limit")]
    pub limit: i64,
}

fn default_limit() -> i64 {
    50
}

impl PaginationParams {
    pub fn effective_limit(&self) -> i64 {
        self.limit.clamp(1, 200)
    }

    pub fn decode_cursor(&self) -> Option<String> {
        self.cursor.as_ref().and_then(|c| {
            URL_SAFE_NO_PAD
                .decode(c)
                .ok()
                .and_then(|bytes| String::from_utf8(bytes).ok())
        })
    }
}

pub fn encode_cursor(value: &str) -> String {
    URL_SAFE_NO_PAD.encode(value)
}

#[derive(Debug, Serialize)]
pub struct PaginatedResponse<T: Serialize> {
    pub items: Vec<T>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
}
