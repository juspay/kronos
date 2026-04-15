pub mod aws;
#[cfg(test)]
pub mod mock;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum KmsProviderType {
    Aws,
    Gcp,
    Vault,
}

impl fmt::Display for KmsProviderType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Aws => write!(f, "aws"),
            Self::Gcp => write!(f, "gcp"),
            Self::Vault => write!(f, "vault"),
        }
    }
}

impl KmsProviderType {
    pub fn from_str_val(s: &str) -> Option<Self> {
        match s {
            "aws" => Some(Self::Aws),
            "gcp" => Some(Self::Gcp),
            "vault" => Some(Self::Vault),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_from_str_val_valid() {
        assert_eq!(KmsProviderType::from_str_val("aws"), Some(KmsProviderType::Aws));
        assert_eq!(KmsProviderType::from_str_val("gcp"), Some(KmsProviderType::Gcp));
        assert_eq!(KmsProviderType::from_str_val("vault"), Some(KmsProviderType::Vault));
    }

    #[test]
    fn test_from_str_val_invalid() {
        assert_eq!(KmsProviderType::from_str_val(""), None);
        assert_eq!(KmsProviderType::from_str_val("AWS"), None);
        assert_eq!(KmsProviderType::from_str_val("azure"), None);
        assert_eq!(KmsProviderType::from_str_val("unknown"), None);
    }

    #[test]
    fn test_display() {
        assert_eq!(KmsProviderType::Aws.to_string(), "aws");
        assert_eq!(KmsProviderType::Gcp.to_string(), "gcp");
        assert_eq!(KmsProviderType::Vault.to_string(), "vault");
    }

    #[test]
    fn test_serde_roundtrip() {
        let provider = KmsProviderType::Aws;
        let json = serde_json::to_string(&provider).unwrap();
        assert_eq!(json, "\"aws\"");
        let parsed: KmsProviderType = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, provider);
    }
}

#[async_trait]
pub trait KmsProvider: Send + Sync {
    /// Fetch the plaintext secret value given its opaque reference (e.g., ARN for AWS).
    async fn get_secret(&self, reference: &str) -> anyhow::Result<String>;

    /// Validate that a reference string is well-formed for this provider.
    fn validate_reference(&self, reference: &str) -> anyhow::Result<()>;

    /// Return the provider type.
    fn provider_type(&self) -> KmsProviderType;
}
