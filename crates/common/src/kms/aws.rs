use async_trait::async_trait;
use aws_sdk_secretsmanager::Client;

pub struct AwsKmsProvider {
    client: Client,
}

impl AwsKmsProvider {
    pub async fn new(
        region: Option<String>,
        endpoint_url: Option<String>,
    ) -> anyhow::Result<Self> {
        let mut config_loader =
            aws_config::defaults(aws_config::BehaviorVersion::latest());
        if let Some(r) = region {
            config_loader = config_loader.region(aws_config::Region::new(r));
        }
        let config = config_loader.load().await;

        let mut sm_config = aws_sdk_secretsmanager::config::Builder::from(&config);
        if let Some(url) = endpoint_url {
            sm_config = sm_config.endpoint_url(url);
        }

        Ok(Self {
            client: Client::from_conf(sm_config.build()),
        })
    }
}

#[async_trait]
impl super::KmsProvider for AwsKmsProvider {
    async fn get_secret(&self, reference: &str) -> anyhow::Result<String> {
        let resp = self
            .client
            .get_secret_value()
            .secret_id(reference)
            .send()
            .await
            .map_err(|e| {
                anyhow::anyhow!(
                    "AWS Secrets Manager fetch failed for '{}': {}",
                    reference,
                    e
                )
            })?;

        resp.secret_string()
            .map(|s| s.to_string())
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Secret '{}' has no string value (binary secrets not supported)",
                    reference
                )
            })
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
