//! Symmetric encryption abstractions for ChaCha20Poly1305 operations
//!
//! Provides unified interfaces for symmetric encryption used throughout Aura.

use chacha20poly1305::{
    aead::{Aead, KeyInit},
    ChaCha20Poly1305, Key, Nonce,
};

use crate::error::CryptoError;

/// ChaCha20Poly1305 encryption key (32 bytes)
pub type ChaCha20Key = Key;

/// ChaCha20Poly1305 nonce (12 bytes)
pub type ChaCha20Nonce = Nonce;

// Note: Random key/nonce generation should use the Effects system
// See generate_random_bytes() in effects.rs for production use

/// Generate a random ChaCha20Poly1305 key (test use only)
#[cfg(test)]
#[allow(clippy::disallowed_types)]
fn generate_chacha20_key() -> ChaCha20Key {
    use rand::{rngs::OsRng, RngCore};
    let mut key_bytes = [0u8; 32];
    OsRng.fill_bytes(&mut key_bytes);
    chacha20_key_from_bytes(&key_bytes)
}

/// Create a ChaCha20Poly1305 key from bytes
pub fn chacha20_key_from_bytes(bytes: &[u8; 32]) -> ChaCha20Key {
    (*bytes).into()
}

/// Create a ChaCha20Poly1305 nonce from bytes
pub fn chacha20_nonce_from_bytes(bytes: &[u8; 12]) -> ChaCha20Nonce {
    (*bytes).into()
}

/// Generate a random ChaCha20Poly1305 nonce
///
/// Note: This uses OsRng directly. For production code with deterministic testing,
/// consider using the Effects system instead.
#[allow(clippy::disallowed_types)]
fn generate_chacha20_nonce() -> ChaCha20Nonce {
    use rand::{rngs::OsRng, RngCore};
    let mut nonce_bytes = [0u8; 12];
    OsRng.fill_bytes(&mut nonce_bytes);
    chacha20_nonce_from_bytes(&nonce_bytes)
}

/// Encrypt data with ChaCha20Poly1305
pub fn chacha20_encrypt(
    key: &ChaCha20Key,
    nonce: &ChaCha20Nonce,
    plaintext: &[u8],
) -> Result<Vec<u8>, CryptoError> {
    let cipher = ChaCha20Poly1305::new(key);
    cipher
        .encrypt(nonce, plaintext)
        .map_err(|e| CryptoError::encryption_failed(e.to_string()))
}

/// Decrypt data with ChaCha20Poly1305
pub fn chacha20_decrypt(
    key: &ChaCha20Key,
    nonce: &ChaCha20Nonce,
    ciphertext: &[u8],
) -> Result<Vec<u8>, CryptoError> {
    let cipher = ChaCha20Poly1305::new(key);
    cipher
        .decrypt(nonce, ciphertext)
        .map_err(|e| CryptoError::decryption_failed(e.to_string()))
}

/// Encrypt data with ChaCha20Poly1305 and include the nonce in the output
pub fn chacha20_encrypt_with_nonce(
    key: &ChaCha20Key,
    plaintext: &[u8],
) -> Result<Vec<u8>, CryptoError> {
    let nonce = generate_chacha20_nonce();
    let ciphertext = chacha20_encrypt(key, &nonce, plaintext)?;

    // Prepend nonce to ciphertext
    let mut result = Vec::with_capacity(12 + ciphertext.len());
    result.extend_from_slice(&nonce);
    result.extend_from_slice(&ciphertext);

    Ok(result)
}

/// Decrypt data with ChaCha20Poly1305 where the nonce is included in the input
pub fn chacha20_decrypt_with_nonce(key: &ChaCha20Key, data: &[u8]) -> Result<Vec<u8>, CryptoError> {
    if data.len() < 12 {
        return Err(CryptoError::decryption_failed(
            "Data too short to contain nonce".to_string(),
        ));
    }

    let (nonce_bytes, ciphertext) = data.split_at(12);
    // Safe: we already verified data.len() >= 12, so nonce_bytes is exactly 12 bytes
    let nonce = chacha20_nonce_from_bytes(
        nonce_bytes
            .try_into()
            .expect("nonce_bytes is exactly 12 bytes"),
    );

    chacha20_decrypt(key, &nonce, ciphertext)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chacha20_key_generation() {
        let key1 = generate_chacha20_key();
        let key2 = generate_chacha20_key();

        // Keys should be different
        assert_ne!(key1, key2);
    }

    #[test]
    fn test_chacha20_encrypt_decrypt() {
        let key = generate_chacha20_key();
        let nonce = generate_chacha20_nonce();
        let plaintext = b"hello world";

        let ciphertext = chacha20_encrypt(&key, &nonce, plaintext).unwrap();
        let decrypted = chacha20_decrypt(&key, &nonce, &ciphertext).unwrap();

        assert_eq!(plaintext.as_slice(), decrypted);
    }

    #[test]
    fn test_chacha20_encrypt_decrypt_with_nonce() {
        let key = generate_chacha20_key();
        let plaintext = b"hello world";

        let encrypted_data = chacha20_encrypt_with_nonce(&key, plaintext).unwrap();
        let decrypted = chacha20_decrypt_with_nonce(&key, &encrypted_data).unwrap();

        assert_eq!(plaintext.as_slice(), decrypted);
    }

    #[test]
    fn test_chacha20_wrong_key_fails() {
        let key1 = generate_chacha20_key();
        let key2 = generate_chacha20_key();
        let plaintext = b"hello world";

        let encrypted_data = chacha20_encrypt_with_nonce(&key1, plaintext).unwrap();
        let result = chacha20_decrypt_with_nonce(&key2, &encrypted_data);

        assert!(result.is_err());
    }
}
