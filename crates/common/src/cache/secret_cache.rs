use dashmap::DashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

pub struct SecretCache {
    cache: Arc<DashMap<String, (String, Instant)>>,
    ttl: Duration,
}

impl SecretCache {
    pub fn new(ttl_secs: u64) -> Self {
        Self {
            cache: Arc::new(DashMap::new()),
            ttl: Duration::from_secs(ttl_secs),
        }
    }

    pub fn get(&self, name: &str) -> Option<String> {
        if let Some(entry) = self.cache.get(name) {
            if entry.1.elapsed() < self.ttl {
                return Some(entry.0.clone());
            }
            drop(entry);
            self.cache.remove(name);
        }
        None
    }

    pub fn set(&self, name: String, value: String) {
        self.cache.insert(name, (value, Instant::now()));
    }

    pub fn invalidate(&self, name: &str) {
        self.cache.remove(name);
    }
}
