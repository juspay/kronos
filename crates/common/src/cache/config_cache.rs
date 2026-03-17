use dashmap::DashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

pub struct ConfigCache {
    cache: Arc<DashMap<String, (serde_json::Value, Instant)>>,
    ttl: Duration,
}

impl ConfigCache {
    pub fn new(ttl_secs: u64) -> Self {
        Self {
            cache: Arc::new(DashMap::new()),
            ttl: Duration::from_secs(ttl_secs),
        }
    }

    pub fn get(&self, name: &str) -> Option<serde_json::Value> {
        if let Some(entry) = self.cache.get(name) {
            if entry.1.elapsed() < self.ttl {
                return Some(entry.0.clone());
            }
            drop(entry);
            self.cache.remove(name);
        }
        None
    }

    pub fn set(&self, name: String, values: serde_json::Value) {
        self.cache.insert(name, (values, Instant::now()));
    }

    pub fn invalidate(&self, name: &str) {
        self.cache.remove(name);
    }
}
