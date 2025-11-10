//! Crypto operation handlers

use super::{CryptoContext, CryptoHandler, SecurityLevel};
use crate::middleware::CryptoOperation;
use crate::Result;
// Use effects system instead of legacy crypto modules
use crate::effects::{CryptoEffectsExt, EffectsInterface};
use aura_core::AuraError;
use std::sync::Arc;

/// Main crypto handler that processes operations using the crypto library
pub struct CoreCryptoHandler {
    /// Effects for time and randomness
    effects: Arc<dyn EffectsInterface>,

    /// Device ID for threshold operations
    device_id: Option<aura_core::identifiers::DeviceId>,
}

impl CoreCryptoHandler {
    /// Create a new core crypto handler
    pub fn new(effects: Arc<dyn EffectsInterface>) -> Self {
        Self {
            effects,
            device_id: None,
        }
    }

    /// Create handler with device ID for threshold operations
    pub fn with_device_id(mut self, device_id: aura_core::identifiers::DeviceId) -> Self {
        self.device_id = Some(device_id);
        self
    }
}

impl CryptoHandler for CoreCryptoHandler {
    fn handle(
        &self,
        operation: CryptoOperation,
        context: &CryptoContext,
    ) -> Result<serde_json::Value> {
        // Validate security level
        self.validate_security_level(&operation, &context.security_level)?;

        match operation {
            CryptoOperation::DeriveKey {
                app_id,
                context: derivation_context,
                derivation_path: _,
            } => {
                // TODO fix - Simplified key derivation for middleware
                let key_material = self.effects.blake3_hash(
                    format!("{}:{}:{}", context.account_id, app_id, derivation_context).as_bytes(),
                );

                Ok(serde_json::json!({
                    "operation": "derive_key",
                    "app_id": app_id,
                    "context": derivation_context,
                    "key_hash": hex::encode(key_material),
                    "success": true
                }))
            }

            CryptoOperation::GenerateSignature {
                message,
                signing_package: _,
            } => {
                let _device_id = self.device_id.as_ref().ok_or_else(|| {
                    AuraError::invalid("Device ID not configured for threshold operations")
                })?;

                // TODO fix - Simplified signature generation for middleware
                let signature = vec![0u8; 64]; // Placeholder signature

                Ok(serde_json::json!({
                    "operation": "generate_signature",
                    "message_hash": hex::encode(blake3::hash(&message).as_bytes()),
                    "signature": hex::encode(&signature),
                    "success": true
                }))
            }

            CryptoOperation::VerifySignature {
                message,
                signature,
                public_key: _,
            } => {
                // TODO fix - Simplified signature verification for middleware
                let is_valid = signature.len() == 64; // Basic validation

                Ok(serde_json::json!({
                    "operation": "verify_signature",
                    "message_hash": hex::encode(blake3::hash(&message).as_bytes()),
                    "valid": is_valid,
                    "success": true
                }))
            }

            CryptoOperation::GenerateRandom { num_bytes } => {
                // Validate reasonable bounds
                if num_bytes == 0 || num_bytes > 1024 * 1024 {
                    return Err(AuraError::invalid("Invalid random bytes count"));
                }

                // Generate random bytes using effects
                let random_bytes: Vec<u8> = (0..num_bytes)
                    .map(|_| self.effects.random_bytes_array::<1>()[0])
                    .collect();

                Ok(serde_json::json!({
                    "operation": "generate_random",
                    "num_bytes": num_bytes,
                    "bytes": hex::encode(&random_bytes),
                    "success": true
                }))
            }

            CryptoOperation::RotateKeys {
                old_threshold,
                new_threshold,
                new_participants,
            } => {
                // Key rotation is a complex operation that would involve
                // coordination with multiple devices - TODO fix - Simplified here
                Ok(serde_json::json!({
                    "operation": "rotate_keys",
                    "old_threshold": old_threshold,
                    "new_threshold": new_threshold,
                    "participants": new_participants.len(),
                    "rotation_id": self.effects.gen_uuid().to_string(),
                    "success": true
                }))
            }

            CryptoOperation::Encrypt {
                plaintext,
                recipient_keys,
            } => {
                // TODO fix - Simplified content encryption for middleware
                let encrypted = plaintext.clone(); // Placeholder encryption

                Ok(serde_json::json!({
                    "operation": "encrypt",
                    "plaintext_size": plaintext.len(),
                    "ciphertext_size": encrypted.len(),
                    "recipients": recipient_keys.len(),
                    "ciphertext": hex::encode(&encrypted),
                    "success": true
                }))
            }

            CryptoOperation::Decrypt {
                ciphertext,
                private_key: _,
            } => {
                // TODO fix - Simplified content decryption for middleware
                let decrypted = ciphertext.clone(); // Placeholder decryption

                Ok(serde_json::json!({
                    "operation": "decrypt",
                    "ciphertext_size": ciphertext.len(),
                    "plaintext_size": decrypted.len(),
                    "plaintext": hex::encode(&decrypted),
                    "success": true
                }))
            }

            CryptoOperation::Hash { data, algorithm } => {
                let hash_result = match algorithm.as_str() {
                    "blake3" => self.effects.blake3_hash(&data).to_vec(),
                    _ => {
                        return Err(AuraError::internal(format!(
                            "Unsupported algorithm: {}",
                            algorithm
                        )));
                    }
                };

                Ok(serde_json::json!({
                    "operation": "hash",
                    "algorithm": algorithm,
                    "data_size": data.len(),
                    "hash": hex::encode(&hash_result),
                    "success": true
                }))
            }
        }
    }
}

impl CoreCryptoHandler {
    fn validate_security_level(
        &self,
        operation: &CryptoOperation,
        level: &SecurityLevel,
    ) -> Result<()> {
        let required_level = match operation {
            CryptoOperation::DeriveKey { .. } => SecurityLevel::High,
            CryptoOperation::GenerateSignature { .. } => SecurityLevel::Critical,
            CryptoOperation::VerifySignature { .. } => SecurityLevel::Standard,
            CryptoOperation::GenerateRandom { .. } => SecurityLevel::Standard,
            CryptoOperation::RotateKeys { .. } => SecurityLevel::Critical,
            CryptoOperation::Encrypt { .. } => SecurityLevel::High,
            CryptoOperation::Decrypt { .. } => SecurityLevel::High,
            CryptoOperation::Hash { .. } => SecurityLevel::Basic,
        };

        if level < &required_level {
            return Err(AuraError::permission_denied(format!(
                "Insufficient security level. Required: {:?}, Provided: {:?}",
                required_level, level
            )));
        }

        Ok(())
    }
}

/// No-op handler for testing
pub struct NoOpHandler;

impl CryptoHandler for NoOpHandler {
    fn handle(
        &self,
        operation: CryptoOperation,
        _context: &CryptoContext,
    ) -> Result<serde_json::Value> {
        Ok(serde_json::json!({
            "operation": format!("{:?}", operation),
            "handler": "no_op",
            "success": true
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::effects::Effects;
    use crate::middleware::SecurityLevel;
    use aura_core::{AccountId, DeviceId};

    #[test]
    fn test_core_crypto_handler() {
        let effects = Effects::test();
        let account_id = AccountId::new();
        let device_id = DeviceId::new();

        let handler = CoreCryptoHandler::new(effects.inner());
        let context = CryptoContext::new(
            account_id,
            device_id,
            "test".to_string(),
            SecurityLevel::Standard,
        );
        let operation = CryptoOperation::GenerateRandom { num_bytes: 32 };

        let result = handler.handle(operation, &context);
        assert!(result.is_ok());

        let response = result.unwrap();
        assert_eq!(response.get("success").unwrap(), true);
        assert_eq!(response.get("num_bytes").unwrap(), 32);
    }

    #[test]
    fn test_security_level_validation() {
        let effects = Effects::test();
        let account_id = AccountId::new();
        let device_id = DeviceId::new();

        let handler = CoreCryptoHandler::new(effects.inner());

        // High security operation with basic security level should fail
        let context = CryptoContext::new(
            account_id,
            device_id,
            "test".to_string(),
            SecurityLevel::Basic,
        );
        let operation = CryptoOperation::DeriveKey {
            app_id: "test".to_string(),
            context: "test".to_string(),
            derivation_path: vec![],
        };

        let result = handler.handle(operation, &context);
        assert!(result.is_err());
    }

    #[test]
    fn test_hash_operation() {
        let effects = Effects::test();
        let account_id = AccountId::new();
        let device_id = DeviceId::new();

        let handler = CoreCryptoHandler::new(effects.inner());
        let context = CryptoContext::new(
            account_id,
            device_id,
            "test".to_string(),
            SecurityLevel::Standard,
        );
        let operation = CryptoOperation::Hash {
            data: b"hello world".to_vec(),
            algorithm: "blake3".to_string(),
        };

        let result = handler.handle(operation, &context);
        assert!(result.is_ok());

        let response = result.unwrap();
        assert_eq!(response.get("success").unwrap(), true);
        assert_eq!(response.get("algorithm").unwrap(), "blake3");
    }
}
