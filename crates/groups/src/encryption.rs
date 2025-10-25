// Causal encryption layer with predecessor key management

use crate::types::*;
use crate::{CgkaError, Result};
use std::collections::BTreeMap;
use tracing::{debug, trace};

/// Causal encryption key with predecessor information
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CausalKey {
    /// Current epoch key
    pub key: Vec<u8>,
    /// Epoch this key is valid for
    pub epoch: Epoch,
    /// Predecessor key for causal encryption
    pub predecessor: Option<Box<CausalKey>>,
    /// Context/purpose this key was derived for
    pub context: String,
}

impl CausalKey {
    /// Create new causal key
    pub fn new(key: Vec<u8>, epoch: Epoch, context: String) -> Self {
        Self {
            key,
            epoch,
            predecessor: None,
            context,
        }
    }

    /// Create causal key with predecessor
    pub fn with_predecessor(
        key: Vec<u8>,
        epoch: Epoch,
        context: String,
        predecessor: CausalKey,
    ) -> Self {
        Self {
            key,
            epoch,
            predecessor: Some(Box::new(predecessor)),
            context,
        }
    }

    /// Get all keys in causal chain (current + predecessors)
    pub fn causal_chain(&self) -> Vec<&Vec<u8>> {
        let mut chain = vec![&self.key];
        let mut current = self.predecessor.as_ref();

        while let Some(pred) = current {
            chain.push(&pred.key);
            current = pred.predecessor.as_ref();
        }

        chain
    }

    /// Get chain length (number of epochs covered)
    pub fn chain_length(&self) -> usize {
        1 + self
            .predecessor
            .as_ref()
            .map(|p| p.chain_length())
            .unwrap_or(0)
    }
}

/// Causal encryption manager
pub struct CausalEncryption {
    /// Application secrets by epoch
    app_secrets: BTreeMap<Epoch, ApplicationSecret>,
    /// Derived keys by context
    context_keys: BTreeMap<String, CausalKey>,
    /// Maximum chain length to maintain
    max_chain_length: usize,
}

impl CausalEncryption {
    /// Create new causal encryption manager
    pub fn new() -> Self {
        Self {
            app_secrets: BTreeMap::new(),
            context_keys: BTreeMap::new(),
            max_chain_length: 10, // Reasonable default
        }
    }

    /// Add application secret for an epoch
    pub fn add_application_secret(&mut self, secret: ApplicationSecret) {
        debug!(
            "Adding application secret for epoch {}",
            secret.epoch.value()
        );
        self.app_secrets.insert(secret.epoch, secret);
    }

    /// Derive causal key for specific context
    pub fn derive_causal_key(&mut self, context: &str, epoch: Epoch) -> Result<CausalKey> {
        let app_secret = self.app_secrets.get(&epoch).ok_or_else(|| {
            CgkaError::InvalidOperation(format!(
                "No application secret for epoch {}",
                epoch.value()
            ))
        })?;

        // Derive key for this context
        let key = app_secret.derive_key(context);

        // Get predecessor key if it exists
        let predecessor = self.context_keys.get(context).cloned();

        // Create causal key
        let causal_key = if let Some(pred) = predecessor {
            // Ensure we don't exceed max chain length
            if pred.chain_length() >= self.max_chain_length {
                debug!(
                    "Truncating causal chain at length {}",
                    self.max_chain_length
                );
                CausalKey::new(key, epoch, context.to_string())
            } else {
                CausalKey::with_predecessor(key, epoch, context.to_string(), pred)
            }
        } else {
            CausalKey::new(key, epoch, context.to_string())
        };

        debug!(
            "Derived causal key for context '{}' at epoch {} (chain length: {})",
            context,
            epoch.value(),
            causal_key.chain_length()
        );

        // Store the new key
        self.context_keys
            .insert(context.to_string(), causal_key.clone());

        Ok(causal_key)
    }

    /// Get current causal key for context
    pub fn get_causal_key(&self, context: &str) -> Option<&CausalKey> {
        self.context_keys.get(context)
    }

    /// Encrypt data with causal encryption
    pub fn encrypt(&self, data: &[u8], context: &str) -> Result<CausalCiphertext> {
        let causal_key = self.get_causal_key(context).ok_or_else(|| {
            CgkaError::InvalidOperation(format!("No causal key for context '{}'", context))
        })?;

        trace!(
            "Encrypting {} bytes with causal key (chain length: {})",
            data.len(),
            causal_key.chain_length()
        );

        // For now, use simple encryption with current key
        // In production, this would use proper AEAD with causal chain
        let ciphertext = self.encrypt_with_key(data, &causal_key.key)?;

        Ok(CausalCiphertext {
            ciphertext,
            epoch: causal_key.epoch,
            context: causal_key.context.clone(),
            chain_length: causal_key.chain_length(),
        })
    }

    /// Decrypt causal ciphertext
    pub fn decrypt(&self, ciphertext: &CausalCiphertext) -> Result<Vec<u8>> {
        let causal_key = self.get_causal_key(&ciphertext.context).ok_or_else(|| {
            CgkaError::InvalidOperation(format!(
                "No causal key for context '{}'",
                ciphertext.context
            ))
        })?;

        trace!(
            "Decrypting ciphertext from epoch {} with current key from epoch {}",
            ciphertext.epoch.value(),
            causal_key.epoch.value()
        );

        // Try current key first
        if let Ok(plaintext) = self.decrypt_with_key(&ciphertext.ciphertext, &causal_key.key) {
            return Ok(plaintext);
        }

        // Try predecessor keys
        let mut current = causal_key.predecessor.as_ref();
        while let Some(pred) = current {
            if let Ok(plaintext) = self.decrypt_with_key(&ciphertext.ciphertext, &pred.key) {
                return Ok(plaintext);
            }
            current = pred.predecessor.as_ref();
        }

        Err(CgkaError::CryptographicError(
            "Failed to decrypt with any key in causal chain".to_string(),
        ))
    }

    /// Encrypt data with AES-GCM using the provided key
    fn encrypt_with_key(&self, data: &[u8], key: &[u8]) -> Result<Vec<u8>> {
        use aes_gcm::aead::{Aead, Payload};
        use aes_gcm::{Aes256Gcm, KeyInit};

        // Ensure key is the correct length for AES-256
        if key.len() != 32 {
            return Err(CgkaError::CryptographicError(
                "Key must be exactly 32 bytes for AES-256-GCM".to_string(),
            ));
        }

        // Create cipher instance
        let cipher = Aes256Gcm::new_from_slice(key)
            .map_err(|e| CgkaError::CryptographicError(format!("Cipher creation failed: {}", e)))?;

        // Generate deterministic nonce from data and key
        // NOTE: For production, nonces should come from injected Effects for proper randomness
        // This deterministic approach is used to avoid disallowed OsRng
        use blake3::Hasher;
        let hash = Hasher::new()
            .update(data)
            .update(key)
            .update(b"aura-causal-encryption-nonce")
            .finalize();
        let mut nonce_bytes = [0u8; 12];
        nonce_bytes.copy_from_slice(&hash.as_bytes()[..12]);
        let nonce = aes_gcm::Nonce::from(nonce_bytes);

        // Encrypt the data
        let payload = Payload {
            msg: data,
            aad: b"aura-causal-key-encryption", // Additional authenticated data
        };

        let ciphertext = cipher.encrypt(&nonce, payload).map_err(|e| {
            CgkaError::CryptographicError(format!("AES-GCM encryption failed: {}", e))
        })?;

        // Combine nonce and ciphertext for storage
        let mut result = Vec::new();
        result.extend_from_slice(&nonce);
        result.extend_from_slice(&ciphertext);

        Ok(result)
    }

    /// Decrypt data with AES-GCM using the provided key
    fn decrypt_with_key(&self, ciphertext: &[u8], key: &[u8]) -> Result<Vec<u8>> {
        use aes_gcm::aead::{Aead, Payload};
        use aes_gcm::{Aes256Gcm, KeyInit};

        // Ensure key is the correct length for AES-256
        if key.len() != 32 {
            return Err(CgkaError::CryptographicError(
                "Key must be exactly 32 bytes for AES-256-GCM".to_string(),
            ));
        }

        // Check minimum length (nonce + at least some ciphertext)
        if ciphertext.len() < 12 {
            // 12 bytes for AES-GCM nonce
            return Err(CgkaError::CryptographicError(
                "Ciphertext too short to contain nonce".to_string(),
            ));
        }

        // Extract nonce and ciphertext
        let (nonce_bytes, encrypted_data) = ciphertext.split_at(12);
        if nonce_bytes.len() != 12 {
            return Err(CgkaError::CryptographicError(
                "Invalid nonce length: expected 12 bytes".to_string(),
            ));
        }
        let mut nonce_array = [0u8; 12];
        nonce_array.copy_from_slice(nonce_bytes);
        let nonce = aes_gcm::Nonce::from(nonce_array);

        // Create cipher instance
        let cipher = Aes256Gcm::new_from_slice(key)
            .map_err(|e| CgkaError::CryptographicError(format!("Cipher creation failed: {}", e)))?;

        // Decrypt the data
        let payload = Payload {
            msg: encrypted_data,
            aad: b"aura-causal-key-encryption", // Must match encryption AAD
        };

        let plaintext = cipher.decrypt(&nonce, payload).map_err(|e| {
            CgkaError::CryptographicError(format!("AES-GCM decryption failed: {}", e))
        })?;

        Ok(plaintext)
    }

    /// Clean up old keys beyond retention policy
    pub fn cleanup_old_keys(&mut self, retain_epochs: usize) {
        if self.app_secrets.len() <= retain_epochs {
            return;
        }

        // Keep only the most recent epochs
        let mut epochs: Vec<_> = self.app_secrets.keys().copied().collect();
        epochs.sort();

        let cutoff_index = epochs.len().saturating_sub(retain_epochs);
        for epoch in epochs.into_iter().take(cutoff_index) {
            self.app_secrets.remove(&epoch);
        }

        debug!(
            "Cleaned up old application secrets, keeping {} epochs",
            retain_epochs
        );
    }
}

impl Default for CausalEncryption {
    fn default() -> Self {
        Self::new()
    }
}

/// Ciphertext with causal encryption metadata
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CausalCiphertext {
    pub ciphertext: Vec<u8>,
    pub epoch: Epoch,
    pub context: String,
    pub chain_length: usize,
}

/// Key derivation utilities
pub mod derivation {
    use super::*;

    /// Derive key for specific purpose from application secret
    pub fn derive_purpose_key(
        app_secret: &ApplicationSecret,
        purpose: &str,
        salt: &[u8],
    ) -> Vec<u8> {
        let mut hasher = blake3::Hasher::new();
        hasher.update(&app_secret.secret);
        hasher.update(purpose.as_bytes());
        hasher.update(salt);
        hasher.update(&app_secret.epoch.0.to_le_bytes());
        hasher.finalize().as_bytes().to_vec()
    }

    /// Derive encryption key for data storage
    pub fn derive_storage_key(app_secret: &ApplicationSecret, storage_id: &str) -> Vec<u8> {
        derive_purpose_key(app_secret, "storage", storage_id.as_bytes())
    }

    /// Derive key for message encryption
    pub fn derive_message_key(app_secret: &ApplicationSecret, channel_id: &str) -> Vec<u8> {
        derive_purpose_key(app_secret, "message", channel_id.as_bytes())
    }

    /// Derive key for capability delegation
    pub fn derive_capability_key(
        app_secret: &ApplicationSecret,
        capability_scope: &str,
    ) -> Vec<u8> {
        derive_purpose_key(app_secret, "capability", capability_scope.as_bytes())
    }
}
