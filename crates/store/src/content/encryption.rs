//! Encryption utilities for storage

use aes_gcm::aead::generic_array::typenum::U12;
use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce as AesNonce,
};
use rand::RngCore;

#[derive(Debug, Clone)]
pub struct Recipients {
    pub device_ids: Vec<Vec<u8>>,
}

impl Recipients {
    pub fn new(device_ids: Vec<Vec<u8>>) -> Self {
        Self { device_ids }
    }
}

pub struct EncryptionContext {
    cipher: Aes256Gcm,
}

impl EncryptionContext {
    pub fn new() -> Self {
        let key = Self::generate_random_key();
        let cipher = Aes256Gcm::new(&key.into());
        Self { cipher }
    }

    pub fn from_key(key: [u8; 32]) -> Self {
        let cipher = Aes256Gcm::new(&key.into());
        Self { cipher }
    }

    pub fn encrypt(&self, plaintext: &[u8]) -> crate::Result<Vec<u8>> {
        let nonce = Self::generate_nonce();
        let ciphertext = self
            .cipher
            .encrypt(&nonce, plaintext)
            .map_err(|_| crate::StoreErrorBuilder::io_error("Encryption failed"))?;

        let mut result = nonce.to_vec();
        result.extend_from_slice(&ciphertext);
        Ok(result)
    }

    pub fn decrypt(&self, data: &[u8]) -> crate::Result<Vec<u8>> {
        if data.len() < 12 {
            return Err(crate::StoreErrorBuilder::io_error("Invalid ciphertext"));
        }

        let (nonce_bytes, ciphertext) = data.split_at(12);
        let nonce = AesNonce::<U12>::from_slice(nonce_bytes);

        self.cipher
            .decrypt(nonce, ciphertext)
            .map_err(|_| crate::StoreErrorBuilder::io_error("Decryption failed"))
    }

    fn generate_random_key() -> [u8; 32] {
        let mut key = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut key);
        key
    }

    fn generate_nonce() -> AesNonce<U12> {
        let mut nonce_bytes = [0u8; 12];
        rand::thread_rng().fill_bytes(&mut nonce_bytes);
        *AesNonce::<U12>::from_slice(&nonce_bytes)
    }
}
