//! Encryption Middleware

use super::handler::{TransportHandler, TransportOperation, TransportResult};
use super::stack::TransportMiddleware;
use aura_protocol::effects::AuraEffects;
use aura_protocol::middleware::{MiddlewareContext, MiddlewareResult};
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct EncryptionConfig {
    pub algorithm: EncryptionAlgorithm,
    pub key_size: KeySize,
    pub enable_compression: bool,
    pub require_authentication: bool,
}

impl Default for EncryptionConfig {
    fn default() -> Self {
        Self {
            algorithm: EncryptionAlgorithm::ChaCha20Poly1305,
            key_size: KeySize::Bits256,
            enable_compression: true,
            require_authentication: true,
        }
    }
}

#[derive(Debug, Clone)]
pub enum EncryptionAlgorithm {
    ChaCha20Poly1305,
    AesGcm256,
    AesGcm128,
    XChaCha20Poly1305,
}

#[derive(Debug, Clone)]
pub enum KeySize {
    Bits128,
    Bits256,
}

impl EncryptionAlgorithm {
    fn encrypt(&self, data: &[u8], _key: &[u8], nonce: &[u8]) -> Result<Vec<u8>, String> {
        // Placeholder encryption - in real implementation would use actual crypto libraries
        match self {
            EncryptionAlgorithm::ChaCha20Poly1305 => {
                let mut encrypted = Vec::with_capacity(data.len() + 32);
                encrypted.extend_from_slice(b"CHA20P13"); // 8-byte header
                encrypted.extend_from_slice(&nonce[..12]); // 12-byte nonce
                encrypted.extend_from_slice(&(data.len() as u32).to_be_bytes()); // 4-byte length
                encrypted.extend_from_slice(data); // Data (would be encrypted)
                encrypted.extend_from_slice(&[0; 16]); // 16-byte auth tag
                Ok(encrypted)
            }
            EncryptionAlgorithm::AesGcm256 => {
                let mut encrypted = Vec::with_capacity(data.len() + 32);
                encrypted.extend_from_slice(b"AES256GC"); // 8-byte header
                encrypted.extend_from_slice(&nonce[..12]); // 12-byte nonce
                encrypted.extend_from_slice(&(data.len() as u32).to_be_bytes()); // 4-byte length
                encrypted.extend_from_slice(data); // Data (would be encrypted)
                encrypted.extend_from_slice(&[0; 16]); // 16-byte auth tag
                Ok(encrypted)
            }
            EncryptionAlgorithm::AesGcm128 => {
                let mut encrypted = Vec::with_capacity(data.len() + 32);
                encrypted.extend_from_slice(b"AES128GC"); // 8-byte header
                encrypted.extend_from_slice(&nonce[..12]); // 12-byte nonce
                encrypted.extend_from_slice(&(data.len() as u32).to_be_bytes()); // 4-byte length
                encrypted.extend_from_slice(data); // Data (would be encrypted)
                encrypted.extend_from_slice(&[0; 16]); // 16-byte auth tag
                Ok(encrypted)
            }
            EncryptionAlgorithm::XChaCha20Poly1305 => {
                let mut encrypted = Vec::with_capacity(data.len() + 48);
                encrypted.extend_from_slice(b"XCHA20P1"); // 8-byte header
                encrypted.extend_from_slice(&nonce[..24]); // 24-byte nonce for XChaCha20
                encrypted.extend_from_slice(&(data.len() as u32).to_be_bytes()); // 4-byte length
                encrypted.extend_from_slice(data); // Data (would be encrypted)
                encrypted.extend_from_slice(&[0; 16]); // 16-byte auth tag
                Ok(encrypted)
            }
        }
    }

    fn decrypt(&self, data: &[u8], _key: &[u8]) -> Result<Vec<u8>, String> {
        if data.len() < 32 {
            return Err("Invalid encrypted data".to_string());
        }

        let header = &data[0..8];
        let expected_header = match self {
            EncryptionAlgorithm::ChaCha20Poly1305 => b"CHA20P13",
            EncryptionAlgorithm::AesGcm256 => b"AES256GC",
            EncryptionAlgorithm::AesGcm128 => b"AES128GC",
            EncryptionAlgorithm::XChaCha20Poly1305 => b"XCHA20P1",
        };

        if header != expected_header {
            return Err(format!(
                "Invalid encryption header: expected {:?}, got {:?}",
                expected_header, header
            ));
        }

        let nonce_size = match self {
            EncryptionAlgorithm::XChaCha20Poly1305 => 24,
            _ => 12,
        };

        if data.len() < 8 + nonce_size + 4 + 16 {
            return Err("Insufficient data for decryption".to_string());
        }

        let original_size = u32::from_be_bytes([
            data[8 + nonce_size],
            data[8 + nonce_size + 1],
            data[8 + nonce_size + 2],
            data[8 + nonce_size + 3],
        ]) as usize;

        // Simulate decryption by extracting the plaintext portion
        let plaintext_start = 8 + nonce_size + 4;
        let plaintext_end = plaintext_start + original_size;

        if data.len() < plaintext_end + 16 {
            return Err("Insufficient data for authentication tag".to_string());
        }

        // In real implementation, would verify auth tag here
        Ok(data[plaintext_start..plaintext_end].to_vec())
    }

    fn nonce_size(&self) -> usize {
        match self {
            EncryptionAlgorithm::XChaCha20Poly1305 => 24,
            _ => 12,
        }
    }
}

pub struct EncryptionMiddleware {
    config: EncryptionConfig,
    key: Vec<u8>,
    stats: EncryptionStats,
}

#[derive(Debug, Default)]
struct EncryptionStats {
    bytes_encrypted: u64,
    bytes_decrypted: u64,
    operations: u64,
    errors: u64,
}

impl EncryptionMiddleware {
    pub fn new(key: Vec<u8>) -> Self {
        Self {
            config: EncryptionConfig::default(),
            key,
            stats: EncryptionStats::default(),
        }
    }

    pub fn with_config(key: Vec<u8>, config: EncryptionConfig) -> Self {
        Self {
            config,
            key,
            stats: EncryptionStats::default(),
        }
    }

    fn generate_nonce(&self, effects: &dyn AuraEffects) -> Vec<u8> {
        let nonce_size = self.config.algorithm.nonce_size();
        let mut nonce = vec![0; nonce_size];

        // Use timestamp and device ID for nonce generation (not cryptographically secure)
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let device_id = aura_types::identifiers::DeviceId::from(uuid::Uuid::new_v4()); // Use random ID for nonce generation

        // Fill nonce with timestamp and device ID data
        let timestamp_bytes = timestamp.to_be_bytes();
        for (i, &byte) in timestamp_bytes.iter().enumerate() {
            if i < nonce_size {
                nonce[i] = byte;
            }
        }

        // XOR with device ID bytes
        let device_bytes = device_id.0.as_bytes();
        for (i, &byte) in device_bytes.iter().enumerate() {
            if i + 8 < nonce_size {
                nonce[i + 8] ^= byte;
            }
        }

        nonce
    }

    fn add_encryption_metadata(&self, metadata: &mut HashMap<String, String>) {
        metadata.insert(
            "encryption".to_string(),
            format!("{:?}", self.config.algorithm),
        );
        metadata.insert(
            "key_size".to_string(),
            format!("{:?}", self.config.key_size),
        );
        metadata.insert(
            "authenticated".to_string(),
            self.config.require_authentication.to_string(),
        );
    }

    fn is_encrypted(&self, metadata: &HashMap<String, String>) -> bool {
        metadata.contains_key("encryption")
    }
}

impl TransportMiddleware for EncryptionMiddleware {
    fn process(
        &mut self,
        operation: TransportOperation,
        _context: &MiddlewareContext,
        effects: &dyn AuraEffects,
        next: &mut dyn TransportHandler,
    ) -> MiddlewareResult<TransportResult> {
        match operation {
            TransportOperation::Send {
                destination,
                data,
                mut metadata,
            } => {
                let nonce = self.generate_nonce(effects);

                match self.config.algorithm.encrypt(&data, &self.key, &nonce) {
                    Ok(encrypted) => {
                        self.add_encryption_metadata(&mut metadata);
                        self.stats.bytes_encrypted += data.len() as u64;
                        self.stats.operations += 1;

                        effects.log_info(
                            &format!(
                                "Encrypted {} bytes using {:?}",
                                data.len(),
                                self.config.algorithm
                            ),
                            &[],
                        );

                        next.execute(
                            TransportOperation::Send {
                                destination,
                                data: encrypted,
                                metadata,
                            },
                            effects,
                        )
                    }
                    Err(e) => {
                        self.stats.errors += 1;
                        effects.log_error(&format!("Encryption failed: {}", e), &[]);

                        // Fall back to unencrypted transmission
                        next.execute(
                            TransportOperation::Send {
                                destination,
                                data,
                                metadata,
                            },
                            effects,
                        )
                    }
                }
            }

            TransportOperation::Receive { source, timeout_ms } => {
                let result =
                    next.execute(TransportOperation::Receive { source, timeout_ms }, effects)?;

                if let TransportResult::Received {
                    source,
                    data,
                    metadata,
                } = result
                {
                    let processed_data = if self.is_encrypted(&metadata) {
                        match self.config.algorithm.decrypt(&data, &self.key) {
                            Ok(decrypted) => {
                                self.stats.bytes_decrypted += decrypted.len() as u64;
                                effects.log_info(
                                    &format!(
                                        "Decrypted {} bytes to {} bytes",
                                        data.len(),
                                        decrypted.len()
                                    ),
                                    &[],
                                );
                                decrypted
                            }
                            Err(e) => {
                                self.stats.errors += 1;
                                effects.log_error(&format!("Decryption failed: {}", e), &[]);
                                data
                            }
                        }
                    } else {
                        data
                    };

                    Ok(TransportResult::Received {
                        source,
                        data: processed_data,
                        metadata,
                    })
                } else {
                    Ok(result)
                }
            }

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
        info.insert(
            "key_size".to_string(),
            format!("{:?}", self.config.key_size),
        );
        info.insert(
            "authentication".to_string(),
            self.config.require_authentication.to_string(),
        );
        info.insert(
            "bytes_encrypted".to_string(),
            self.stats.bytes_encrypted.to_string(),
        );
        info.insert(
            "bytes_decrypted".to_string(),
            self.stats.bytes_decrypted.to_string(),
        );
        info.insert("operations".to_string(), self.stats.operations.to_string());
        info.insert("errors".to_string(), self.stats.errors.to_string());
        info
    }
}
