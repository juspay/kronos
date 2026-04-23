use aws_sdk_kms::{
    config::{BehaviorVersion, Credentials, Region},
    primitives::Blob,
    Client,
};
use base64::{engine::general_purpose, Engine};

async fn decrypt_helper(client: &Client, key: &str, encoded_value: String) -> String {
    let decoded = general_purpose::STANDARD
        .decode(encoded_value)
        .unwrap_or_else(|_| panic!("{key}: input is not valid base64"));

    let result = client
        .decrypt()
        .ciphertext_blob(Blob::new(decoded))
        .send()
        .await;

    String::from_utf8(
        result
            .unwrap_or_else(|_| panic!("Failed to KMS-decrypt {key}"))
            .plaintext()
            .unwrap_or_else(|| panic!("No plaintext returned for {key}"))
            .as_ref()
            .to_vec(),
    )
    .expect("KMS-decrypted value is not valid UTF-8")
}

/// Decrypt a KMS-encrypted, base64-encoded environment variable. Panics if the var is missing.
pub async fn decrypt(client: &Client, key: &str) -> String {
    let value = std::env::var(key).unwrap_or_else(|_| panic!("{key} not present in env"));
    decrypt_helper(client, key, value).await
}

/// Decrypt a KMS-encrypted, base64-encoded environment variable. Returns None if the var is missing.
pub async fn decrypt_opt(client: &Client, key: &str) -> Option<String> {
    let value = std::env::var(key).ok()?;
    Some(decrypt_helper(client, key, value).await)
}

/// Create a new AWS KMS client.
///
/// When `AWS_ACCESS_KEY_ID` and `AWS_SECRET_ACCESS_KEY` are set, builds the
/// client directly from those values (fast, no network discovery).
/// Otherwise falls back to the full credential chain (`aws_config::from_env`)
/// which supports IAM roles, IMDS, etc.
///
/// Reads `AWS_ENDPOINT_URL` to support LocalStack or other custom endpoints.
pub async fn new_client() -> Client {
    let region = Region::new(
        std::env::var("AWS_REGION").unwrap_or_else(|_| "us-east-1".to_string()),
    );
    let endpoint = std::env::var("AWS_ENDPOINT_URL").ok();

    if let Some(ref ep) = endpoint {
        tracing::info!("Using custom AWS endpoint: {ep}");
    }

    // Fast path: explicit static credentials from env
    if let (Ok(access_key), Ok(secret_key)) = (
        std::env::var("AWS_ACCESS_KEY_ID"),
        std::env::var("AWS_SECRET_ACCESS_KEY"),
    ) {
        let creds =
            Credentials::new(access_key, secret_key, None, None, "env");

        let mut builder = aws_sdk_kms::config::Builder::new()
            .behavior_version(BehaviorVersion::latest())
            .region(region)
            .credentials_provider(creds);

        if let Some(ep) = endpoint {
            builder = builder.endpoint_url(ep);
        }

        return Client::from_conf(builder.build());
    }

    // Slow path: full credential chain (IAM roles, IMDS, etc.)
    let mut loader = aws_config::from_env().region(region);
    if let Some(ep) = endpoint {
        loader = loader.endpoint_url(ep);
    }
    let config = loader.load().await;
    Client::new(&config)
}
