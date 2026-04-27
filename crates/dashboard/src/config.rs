use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DashboardConfig {
    pub api_base_url: String,
    pub api_prefix: String,
    pub dashboard_prefix: String,
    pub api_key: String,
}

impl DashboardConfig {
    pub fn api_base(&self) -> String {
        if self.api_base_url.is_empty() {
            self.api_prefix.clone()
        } else {
            format!(
                "{}{}",
                self.api_base_url.trim_end_matches('/'),
                self.api_prefix
            )
        }
    }
}
