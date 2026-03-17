use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce,
};
use rand::RngCore;

pub fn encrypt(plaintext: &str, key_hex: &str) -> anyhow::Result<Vec<u8>> {
    let key_bytes = hex_decode(key_hex)?;
    let cipher = Aes256Gcm::new_from_slice(&key_bytes)
        .map_err(|e| anyhow::anyhow!("Invalid key length: {}", e))?;

    let mut nonce_bytes = [0u8; 12];
    rand::thread_rng().fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);

    let ciphertext = cipher
        .encrypt(nonce, plaintext.as_bytes())
        .map_err(|e| anyhow::anyhow!("Encryption failed: {}", e))?;

    // Prepend nonce to ciphertext
    let mut result = nonce_bytes.to_vec();
    result.extend_from_slice(&ciphertext);
    Ok(result)
}

pub fn decrypt(encrypted: &[u8], key_hex: &str) -> anyhow::Result<String> {
    if encrypted.len() < 12 {
        anyhow::bail!("Encrypted data too short");
    }

    let key_bytes = hex_decode(key_hex)?;
    let cipher = Aes256Gcm::new_from_slice(&key_bytes)
        .map_err(|e| anyhow::anyhow!("Invalid key length: {}", e))?;

    let (nonce_bytes, ciphertext) = encrypted.split_at(12);
    let nonce = Nonce::from_slice(nonce_bytes);

    let plaintext = cipher
        .decrypt(nonce, ciphertext)
        .map_err(|e| anyhow::anyhow!("Decryption failed: {}", e))?;

    Ok(String::from_utf8(plaintext)?)
}

fn hex_decode(hex: &str) -> anyhow::Result<Vec<u8>> {
    if hex.len() != 64 {
        anyhow::bail!("Encryption key must be 64 hex chars (32 bytes)");
    }
    (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).map_err(Into::into))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encrypt_decrypt() {
        let key = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let plaintext = "my-secret-value";
        let encrypted = encrypt(plaintext, key).unwrap();
        let decrypted = decrypt(&encrypted, key).unwrap();
        assert_eq!(decrypted, plaintext);
    }
}
