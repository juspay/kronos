use aws_sdk_kms::{primitives::Blob, Client};
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

/// Create a new AWS KMS client using credentials from the environment.
pub async fn new_client() -> Client {
    let config = aws_config::load_from_env().await;
    Client::new(&config)
}
