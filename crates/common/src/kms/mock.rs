use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

/// In-memory KMS provider for testing.
/// Pre-load it with secrets via `with_secret()` and track fetch counts.
pub struct MockKmsProvider {
    secrets: Arc<HashMap<String, String>>,
    fetch_count: Arc<AtomicU64>,
}

impl MockKmsProvider {
    pub fn new() -> Self {
        Self {
            secrets: Arc::new(HashMap::new()),
            fetch_count: Arc::new(AtomicU64::new(0)),
        }
    }

    pub fn with_secret(mut self, reference: &str, value: &str) -> Self {
        Arc::get_mut(&mut self.secrets)
            .expect("MockKmsProvider::with_secret called after clone")
            .insert(reference.to_string(), value.to_string());
        self
    }

    /// How many times `get_secret` was called.
    pub fn fetch_count(&self) -> u64 {
        self.fetch_count.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl super::KmsProvider for MockKmsProvider {
    async fn get_secret(&self, reference: &str) -> anyhow::Result<String> {
        self.fetch_count.fetch_add(1, Ordering::SeqCst);
        self.secrets
            .get(reference)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("Mock secret not found: {}", reference))
    }

    fn validate_reference(&self, reference: &str) -> anyhow::Result<()> {
        if reference.is_empty() {
            anyhow::bail!("Reference cannot be empty");
        }
        Ok(())
    }

    fn provider_type(&self) -> super::KmsProviderType {
        super::KmsProviderType::Aws
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cache::SecretCache;
    use crate::kms::KmsProvider;

    #[tokio::test]
    async fn test_mock_provider_returns_secret() {
        let provider = MockKmsProvider::new()
            .with_secret("arn:aws:sm:us-east-1:123:secret:api-key", "sk-live-abc123");

        let value = provider
            .get_secret("arn:aws:sm:us-east-1:123:secret:api-key")
            .await
            .unwrap();
        assert_eq!(value, "sk-live-abc123");
        assert_eq!(provider.fetch_count(), 1);
    }

    #[tokio::test]
    async fn test_mock_provider_missing_secret() {
        let provider = MockKmsProvider::new();
        let result = provider.get_secret("nonexistent").await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("nonexistent"));
    }

    #[tokio::test]
    async fn test_cache_hit_skips_kms() {
        let provider = MockKmsProvider::new()
            .with_secret("my-ref", "secret-value");

        let cache = SecretCache::new(300);

        // Cache miss: should call KMS
        assert!(cache.get("my-secret").is_none());
        let value = provider.get_secret("my-ref").await.unwrap();
        cache.set("my-secret".to_string(), value.clone());
        assert_eq!(provider.fetch_count(), 1);

        // Cache hit: no KMS call needed
        let cached = cache.get("my-secret").unwrap();
        assert_eq!(cached, "secret-value");
        assert_eq!(provider.fetch_count(), 1); // still 1

        // Invalidate cache → next get returns None
        cache.invalidate("my-secret");
        assert!(cache.get("my-secret").is_none());

        // Re-fetch from KMS
        let value2 = provider.get_secret("my-ref").await.unwrap();
        cache.set("my-secret".to_string(), value2);
        assert_eq!(provider.fetch_count(), 2);
    }

    #[tokio::test]
    async fn test_cache_ttl_expiry_triggers_refetch() {
        let provider = MockKmsProvider::new()
            .with_secret("ref-1", "value-1");

        // 0-second TTL = always expired
        let cache = SecretCache::new(0);

        let value = provider.get_secret("ref-1").await.unwrap();
        cache.set("key".to_string(), value);
        assert_eq!(provider.fetch_count(), 1);

        // Even though we just set it, TTL=0 means it's already expired
        assert!(cache.get("key").is_none());

        // Would need to re-fetch
        let _value2 = provider.get_secret("ref-1").await.unwrap();
        assert_eq!(provider.fetch_count(), 2);
    }

    #[test]
    fn test_validate_reference_empty() {
        let provider = MockKmsProvider::new();
        let result = provider.validate_reference("");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("empty"));
    }

    #[test]
    fn test_validate_reference_valid() {
        let provider = MockKmsProvider::new();
        assert!(provider.validate_reference("arn:aws:sm:us-east-1:123:secret:key").is_ok());
        assert!(provider.validate_reference("any-non-empty-string").is_ok());
    }

    #[tokio::test]
    async fn test_multiple_secrets() {
        let provider = MockKmsProvider::new()
            .with_secret("ref-api-key", "sk-123")
            .with_secret("ref-db-pass", "pg-secret-456");

        let v1 = provider.get_secret("ref-api-key").await.unwrap();
        let v2 = provider.get_secret("ref-db-pass").await.unwrap();
        assert_eq!(v1, "sk-123");
        assert_eq!(v2, "pg-secret-456");
        assert_eq!(provider.fetch_count(), 2);
    }
}
