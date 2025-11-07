//! Encryption Middleware
//!
//! Provides transparent encryption/decryption of stored data.

use super::handler::{StorageError, StorageHandler, StorageOperation, StorageResult};
use super::stack::StorageMiddleware;
use aura_protocol::effects::AuraEffects;
use aura_protocol::middleware::{MiddlewareContext, MiddlewareError, MiddlewareResult};
use aura_types::AuraError;
use std::collections::HashMap;

/// Configuration for encryption middleware
#[derive(Debug, Clone)]
pub struct EncryptionConfig {
    pub key_id: String,
    pub algorithm: EncryptionAlgorithm,
    pub compress_before_encrypt: bool,
}

/// Supported encryption algorithms
#[derive(Debug, Clone)]
pub enum EncryptionAlgorithm {
    AesGcm256,
    ChaCha20Poly1305,
    XChaCha20Poly1305,
}

impl Default for EncryptionConfig {
    fn default() -> Self {
        Self {
            key_id: "default".to_string(),
            algorithm: EncryptionAlgorithm::AesGcm256,
            compress_before_encrypt: true,
        }
    }
}

/// Encryption middleware that encrypts data before storage and decrypts on retrieval
pub struct EncryptionMiddleware {
    config: EncryptionConfig,
    key_cache: HashMap<String, Vec<u8>>, // Simple key cache (in production, use secure key management)
}

impl EncryptionMiddleware {
    /// Create new encryption middleware with default configuration
    pub fn new() -> Self {
        Self {
            config: EncryptionConfig::default(),
            key_cache: HashMap::new(),
        }
    }

    /// Create new encryption middleware with custom configuration
    pub fn with_config(config: EncryptionConfig) -> Self {
        Self {
            config,
            key_cache: HashMap::new(),
        }
    }

    /// Add an encryption key to the middleware
    pub fn add_key(mut self, key_id: String, key: Vec<u8>) -> Self {
        self.key_cache.insert(key_id, key);
        self
    }

    /// Encrypt data using the configured algorithm
    fn encrypt_data(&self, data: &[u8]) -> Result<Vec<u8>, StorageError> {
        // Placeholder encryption implementation
        // In production, this would use actual cryptographic libraries
        let key = self.key_cache.get(&self.config.key_id).ok_or_else(|| {
            StorageError::EncryptionError {
                message: format!("Encryption key not found: {}", self.config.key_id),
            }
        })?;

        // Simple XOR encryption for demonstration (NOT secure)
        let mut encrypted = Vec::with_capacity(data.len() + 16); // 16 bytes for IV/nonce

        // Add a placeholder IV/nonce
        encrypted.extend_from_slice(&[0u8; 16]);

        // Simple XOR with first key byte (demonstration only)
        if let Some(&key_byte) = key.first() {
            for &byte in data {
                encrypted.push(byte ^ key_byte);
            }
        } else {
            return Err(StorageError::EncryptionError {
                message: "Invalid encryption key".to_string(),
            });
        }

        Ok(encrypted)
    }

    /// Decrypt data using the configured algorithm
    fn decrypt_data(&self, encrypted_data: &[u8]) -> Result<Vec<u8>, StorageError> {
        if encrypted_data.len() < 16 {
            return Err(StorageError::EncryptionError {
                message: "Invalid encrypted data format".to_string(),
            });
        }

        let key = self.key_cache.get(&self.config.key_id).ok_or_else(|| {
            StorageError::EncryptionError {
                message: format!("Decryption key not found: {}", self.config.key_id),
            }
        })?;

        // Skip the IV/nonce (first 16 bytes)
        let ciphertext = &encrypted_data[16..];
        let mut decrypted = Vec::with_capacity(ciphertext.len());

        // Simple XOR decryption (demonstration only)
        if let Some(&key_byte) = key.first() {
            for &byte in ciphertext {
                decrypted.push(byte ^ key_byte);
            }
        } else {
            return Err(StorageError::EncryptionError {
                message: "Invalid decryption key".to_string(),
            });
        }

        Ok(decrypted)
    }
}

impl Default for EncryptionMiddleware {
    fn default() -> Self {
        Self::new()
    }
}

impl StorageMiddleware for EncryptionMiddleware {
    fn process(
        &mut self,
        operation: StorageOperation,
        _context: &MiddlewareContext,
        effects: &dyn AuraEffects,
        next: &mut dyn StorageHandler,
    ) -> MiddlewareResult<StorageResult> {
        match operation {
            StorageOperation::Store {
                chunk_id,
                data,
                mut metadata,
            } => {
                // Encrypt the data before passing to next handler
                let encrypted_data =
                    self.encrypt_data(&data)
                        .map_err(|e| MiddlewareError::General {
                            message: format!("Encryption failed: {}", e),
                        })?;

                // Add encryption metadata
                metadata.insert("encrypted".to_string(), "true".to_string());
                metadata.insert(
                    "encryption_algorithm".to_string(),
                    format!("{:?}", self.config.algorithm),
                );
                metadata.insert("encryption_key_id".to_string(), self.config.key_id.clone());

                let encrypted_operation = StorageOperation::Store {
                    chunk_id,
                    data: encrypted_data,
                    metadata,
                };

                next.execute(encrypted_operation, effects)
            }

            StorageOperation::Retrieve { chunk_id: _ } => {
                // Retrieve the encrypted data first
                let result = next.execute(operation, effects)?;

                match result {
                    StorageResult::Retrieved {
                        chunk_id: retrieved_chunk_id,
                        data,
                        metadata,
                    } => {
                        // Check if data is encrypted
                        if metadata
                            .get("encrypted")
                            .map(|v| v == "true")
                            .unwrap_or(false)
                        {
                            // Decrypt the data
                            let decrypted_data =
                                self.decrypt_data(&data)
                                    .map_err(|e| MiddlewareError::General {
                                        message: format!("Decryption failed: {}", e),
                                    })?;

                            Ok(StorageResult::Retrieved {
                                chunk_id: retrieved_chunk_id,
                                data: decrypted_data,
                                metadata,
                            })
                        } else {
                            // Data is not encrypted, return as-is
                            Ok(StorageResult::Retrieved {
                                chunk_id: retrieved_chunk_id,
                                data,
                                metadata,
                            })
                        }
                    }
                    _ => Ok(result),
                }
            }

            // For other operations, pass through unchanged
            _ => next.execute(operation, effects),
        }
    }

    fn middleware_name(&self) -> &'static str {
        "EncryptionMiddleware"
    }

    fn middleware_info(&self) -> HashMap<String, String> {
        let mut info = HashMap::new();
        info.insert(
            "algorithm".to_string(),
            format!("{:?}", self.config.algorithm),
        );
        info.insert("key_id".to_string(), self.config.key_id.clone());
        info.insert(
            "compress_before_encrypt".to_string(),
            self.config.compress_before_encrypt.to_string(),
        );
        info.insert("cached_keys".to_string(), self.key_cache.len().to_string());
        info
    }

    fn initialize(&mut self, _context: &MiddlewareContext) -> MiddlewareResult<()> {
        // In production, this would load keys from secure key management system
        if self.key_cache.is_empty() {
            // Add a default key for demonstration
            let default_key = vec![0x42; 32]; // NOT secure - just for demonstration
            self.key_cache
                .insert(self.config.key_id.clone(), default_key);
        }

        Ok(())
    }
}

/// Builder for encryption middleware
pub struct EncryptionBuilder {
    config: EncryptionConfig,
    keys: HashMap<String, Vec<u8>>,
}

impl EncryptionBuilder {
    pub fn new() -> Self {
        Self {
            config: EncryptionConfig::default(),
            keys: HashMap::new(),
        }
    }

    pub fn with_algorithm(mut self, algorithm: EncryptionAlgorithm) -> Self {
        self.config.algorithm = algorithm;
        self
    }

    pub fn with_key_id(mut self, key_id: String) -> Self {
        self.config.key_id = key_id;
        self
    }

    pub fn add_key(mut self, key_id: String, key: Vec<u8>) -> Self {
        self.keys.insert(key_id, key);
        self
    }

    pub fn compress_before_encrypt(mut self, enable: bool) -> Self {
        self.config.compress_before_encrypt = enable;
        self
    }

    pub fn build(self) -> EncryptionMiddleware {
        let mut middleware = EncryptionMiddleware::with_config(self.config);
        for (key_id, key) in self.keys {
            middleware = middleware.add_key(key_id, key);
        }
        middleware
    }
}

impl Default for EncryptionBuilder {
    fn default() -> Self {
        Self::new()
    }
}
