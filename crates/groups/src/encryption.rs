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
    pub fn with_predecessor(key: Vec<u8>, epoch: Epoch, context: String, predecessor: CausalKey) -> Self {
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
        1 + self.predecessor.as_ref().map(|p| p.chain_length()).unwrap_or(0)
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
        debug!("Adding application secret for epoch {}", secret.epoch.value());
        self.app_secrets.insert(secret.epoch, secret);
    }
    
    /// Derive causal key for specific context
    pub fn derive_causal_key(&mut self, context: &str, epoch: Epoch) -> Result<CausalKey> {
        let app_secret = self.app_secrets.get(&epoch)
            .ok_or_else(|| CgkaError::InvalidOperation(format!("No application secret for epoch {}", epoch.value())))?;
        
        // Derive key for this context
        let key = app_secret.derive_key(context);
        
        // Get predecessor key if it exists
        let predecessor = self.context_keys.get(context).cloned();
        
        // Create causal key
        let causal_key = if let Some(pred) = predecessor {
            // Ensure we don't exceed max chain length
            if pred.chain_length() >= self.max_chain_length {
                debug!("Truncating causal chain at length {}", self.max_chain_length);
                CausalKey::new(key, epoch, context.to_string())
            } else {
                CausalKey::with_predecessor(key, epoch, context.to_string(), pred)
            }
        } else {
            CausalKey::new(key, epoch, context.to_string())
        };
        
        debug!("Derived causal key for context '{}' at epoch {} (chain length: {})", 
               context, epoch.value(), causal_key.chain_length());
        
        // Store the new key
        self.context_keys.insert(context.to_string(), causal_key.clone());
        
        Ok(causal_key)
    }
    
    /// Get current causal key for context
    pub fn get_causal_key(&self, context: &str) -> Option<&CausalKey> {
        self.context_keys.get(context)
    }
    
    /// Encrypt data with causal encryption
    pub fn encrypt(&self, data: &[u8], context: &str) -> Result<CausalCiphertext> {
        let causal_key = self.get_causal_key(context)
            .ok_or_else(|| CgkaError::InvalidOperation(format!("No causal key for context '{}'", context)))?;
        
        trace!("Encrypting {} bytes with causal key (chain length: {})", data.len(), causal_key.chain_length());
        
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
        let causal_key = self.get_causal_key(&ciphertext.context)
            .ok_or_else(|| CgkaError::InvalidOperation(format!("No causal key for context '{}'", ciphertext.context)))?;
        
        trace!("Decrypting ciphertext from epoch {} with current key from epoch {}", 
               ciphertext.epoch.value(), causal_key.epoch.value());
        
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
        
        Err(CgkaError::CryptographicError("Failed to decrypt with any key in causal chain".to_string()))
    }
    
    /// Simple encryption with a single key (placeholder)
    fn encrypt_with_key(&self, data: &[u8], key: &[u8]) -> Result<Vec<u8>> {
        // Placeholder: In production this would use AES-GCM or similar
        let mut result = Vec::new();
        for (i, &byte) in data.iter().enumerate() {
            result.push(byte ^ key[i % key.len()]);
        }
        Ok(result)
    }
    
    /// Simple decryption with a single key (placeholder)
    fn decrypt_with_key(&self, ciphertext: &[u8], key: &[u8]) -> Result<Vec<u8>> {
        // Placeholder: In production this would use AES-GCM or similar
        let mut result = Vec::new();
        for (i, &byte) in ciphertext.iter().enumerate() {
            result.push(byte ^ key[i % key.len()]);
        }
        Ok(result)
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
        
        debug!("Cleaned up old application secrets, keeping {} epochs", retain_epochs);
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
    pub fn derive_purpose_key(app_secret: &ApplicationSecret, purpose: &str, salt: &[u8]) -> Vec<u8> {
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
    pub fn derive_capability_key(app_secret: &ApplicationSecret, capability_scope: &str) -> Vec<u8> {
        derive_purpose_key(app_secret, "capability", capability_scope.as_bytes())
    }
}