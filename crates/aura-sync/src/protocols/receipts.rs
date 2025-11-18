//! Receipt verification protocol
//!
//! Provides cryptographic receipt verification for multi-hop message chains
//! and attestation verification for distributed operations.
//!
//! # Usage
//!
//! ```rust,no_run
//! use aura_sync::protocols::{ReceiptVerificationProtocol, ReceiptVerificationConfig};
//!
//! # fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let config = ReceiptVerificationConfig::default();
//! let protocol = ReceiptVerificationProtocol::new(config);
//!
//! // Verify receipt chain
//! let result = protocol.verify_receipt_chain(&receipts)?;
//! assert!(result.valid);
//! # Ok(())
//! # }
//! ```

use serde::{Deserialize, Serialize};

use crate::core::{sync_session_error, SyncResult};
use aura_core::effects::CryptoEffects;
use aura_core::{DeviceId, Hash32};

// =============================================================================
// Types
// =============================================================================

/// Cryptographic receipt
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Receipt {
    /// Message hash being receipted
    pub message_hash: Hash32,

    /// Signing device
    pub signer: DeviceId,

    /// Public key used for signature verification
    pub public_key: Vec<u8>,

    /// Signature over message hash
    pub signature: Vec<u8>,

    /// Receipt timestamp
    pub timestamp: u64,

    /// Optional previous receipt in chain
    pub previous_receipt: Option<Box<Receipt>>,
}

/// Receipt chain verification result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationResult {
    /// Whether chain is valid
    pub valid: bool,

    /// Number of receipts verified
    pub receipts_verified: usize,

    /// Chain depth
    pub chain_depth: usize,

    /// Devices in chain
    pub signers: Vec<DeviceId>,

    /// Error if verification failed
    pub error: Option<String>,
}

// =============================================================================
// Configuration
// =============================================================================

/// Receipt verification configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReceiptVerificationConfig {
    /// Maximum chain depth to verify
    pub max_chain_depth: usize,

    /// Require chronological ordering
    pub require_chronological: bool,

    /// Enable signature verification
    pub verify_signatures: bool,
}

impl Default for ReceiptVerificationConfig {
    fn default() -> Self {
        Self {
            max_chain_depth: 100,
            require_chronological: true,
            verify_signatures: true,
        }
    }
}

// =============================================================================
// Receipt Verification Protocol
// =============================================================================

/// Receipt verification protocol
pub struct ReceiptVerificationProtocol {
    config: ReceiptVerificationConfig,
}

impl ReceiptVerificationProtocol {
    /// Create a new receipt verification protocol
    pub fn new(config: ReceiptVerificationConfig) -> Self {
        Self { config }
    }

    /// Verify a single receipt using cryptographic verification
    pub async fn verify_receipt<E>(&self, receipt: &Receipt, crypto_effects: &E) -> SyncResult<bool>
    where
        E: CryptoEffects + Send + Sync,
    {
        // Basic validation
        if receipt.signature.is_empty() {
            return Err(sync_session_error("Receipt has empty signature"));
        }

        if receipt.public_key.is_empty() {
            return Err(sync_session_error("Receipt has empty public key"));
        }

        // Skip cryptographic verification if disabled in config
        if !self.config.verify_signatures {
            return Ok(true);
        }

        // Verify the signature using CryptoEffects
        tracing::debug!(
            "Verifying receipt signature for device {} over message hash {:?}",
            receipt.signer,
            receipt.message_hash
        );

        // Prepare the signed data (message hash + timestamp for replay protection)
        let mut signed_data = Vec::with_capacity(32 + 8);
        signed_data.extend_from_slice(receipt.message_hash.as_ref());
        signed_data.extend_from_slice(&receipt.timestamp.to_le_bytes());

        // Use CryptoEffects to verify the signature with Ed25519
        let is_valid = crypto_effects
            .ed25519_verify(&signed_data, &receipt.signature, &receipt.public_key)
            .await
            .map_err(|e| sync_session_error(&format!("Ed25519 verification failed: {}", e)))?;

        if !is_valid {
            tracing::warn!(
                "Receipt signature verification failed for device {} message {:?}",
                receipt.signer,
                receipt.message_hash
            );
            return Ok(false);
        }

        tracing::debug!(
            "Successfully verified receipt signature for device {}",
            receipt.signer
        );

        Ok(true)
    }

    /// Verify a receipt chain using cryptographic verification
    pub async fn verify_receipt_chain<E>(
        &self,
        receipts: &[Receipt],
        crypto_effects: &E,
    ) -> SyncResult<VerificationResult>
    where
        E: CryptoEffects + Send + Sync,
    {
        if receipts.is_empty() {
            return Ok(VerificationResult {
                valid: true,
                receipts_verified: 0,
                chain_depth: 0,
                signers: Vec::new(),
                error: None,
            });
        }

        if receipts.len() > self.config.max_chain_depth {
            return Ok(VerificationResult {
                valid: false,
                receipts_verified: 0,
                chain_depth: receipts.len(),
                signers: Vec::new(),
                error: Some("Chain exceeds maximum depth".to_string()),
            });
        }

        let mut signers = Vec::new();
        let mut last_timestamp = 0;

        for receipt in receipts {
            // Verify individual receipt cryptographically
            if !self.verify_receipt(receipt, crypto_effects).await? {
                return Ok(VerificationResult {
                    valid: false,
                    receipts_verified: signers.len(),
                    chain_depth: receipts.len(),
                    signers,
                    error: Some("Receipt signature verification failed".to_string()),
                });
            }

            // Check chronological ordering
            if self.config.require_chronological && receipt.timestamp < last_timestamp {
                return Ok(VerificationResult {
                    valid: false,
                    receipts_verified: signers.len(),
                    chain_depth: receipts.len(),
                    signers,
                    error: Some("Receipts not in chronological order".to_string()),
                });
            }

            // Verify chain linkage if this receipt references a previous one
            if let Some(ref prev) = receipt.previous_receipt {
                if prev.timestamp > receipt.timestamp {
                    return Ok(VerificationResult {
                        valid: false,
                        receipts_verified: signers.len(),
                        chain_depth: receipts.len(),
                        signers,
                        error: Some("Previous receipt has later timestamp".to_string()),
                    });
                }

                // Verify the previous receipt too
                if !self.verify_receipt(prev, crypto_effects).await? {
                    return Ok(VerificationResult {
                        valid: false,
                        receipts_verified: signers.len(),
                        chain_depth: receipts.len(),
                        signers,
                        error: Some("Previous receipt verification failed".to_string()),
                    });
                }
            }

            signers.push(receipt.signer);
            last_timestamp = receipt.timestamp;
        }

        Ok(VerificationResult {
            valid: true,
            receipts_verified: receipts.len(),
            chain_depth: receipts.len(),
            signers,
            error: None,
        })
    }

    /// Create a cryptographically signed receipt
    pub async fn create_receipt<E>(
        &self,
        message_hash: Hash32,
        signer: DeviceId,
        crypto_effects: &E,
        previous_receipt: Option<Box<Receipt>>,
    ) -> SyncResult<Receipt>
    where
        E: CryptoEffects + Send + Sync,
    {
        // Get current timestamp (in production, this would use TimeEffects)
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| sync_session_error(&format!("Failed to get timestamp: {}", e)))?
            .as_secs();

        // Generate an Ed25519 key pair for signing (in production, this would retrieve the device's key)
        let (public_key, private_key) =
            crypto_effects
                .ed25519_generate_keypair()
                .await
                .map_err(|e| {
                    sync_session_error(&format!("Failed to generate Ed25519 keypair: {}", e))
                })?;

        // Prepare the signed data (message hash + timestamp for replay protection)
        let mut signed_data = Vec::with_capacity(32 + 8);
        signed_data.extend_from_slice(message_hash.as_ref());
        signed_data.extend_from_slice(&timestamp.to_le_bytes());

        // Create Ed25519 signature
        let signature = crypto_effects
            .ed25519_sign(&signed_data, &private_key)
            .await
            .map_err(|e| {
                sync_session_error(&format!("Failed to create Ed25519 signature: {}", e))
            })?;

        tracing::debug!(
            "Created receipt for device {} over message {:?} at timestamp {}",
            signer,
            message_hash,
            timestamp
        );

        Ok(Receipt {
            message_hash,
            signer,
            public_key,
            signature,
            timestamp,
            previous_receipt,
        })
    }
}

impl Default for ReceiptVerificationProtocol {
    fn default() -> Self {
        Self::new(ReceiptVerificationConfig::default())
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use aura_core::effects::{
        CryptoError, FrostKeyGenResult, FrostSigningPackage, KeyDerivationContext,
    };

    // Mock crypto effects for testing
    #[derive(Debug)]
    struct MockCryptoEffects;

    #[async_trait]
    impl CryptoEffects for MockCryptoEffects {
        // Inherit from RandomEffects
        async fn random_bytes(&self, len: usize) -> Result<Vec<u8>, CryptoError> {
            Ok(vec![1; len])
        }

        async fn random_bytes_32(&self) -> Result<[u8; 32], CryptoError> {
            Ok([1; 32])
        }

        async fn random_range(&self, min: u64, max: u64) -> Result<u64, CryptoError> {
            Ok((min + max) / 2) // Simple deterministic value
        }

        // HKDF key derivation
        async fn hkdf_derive(
            &self,
            _ikm: &[u8],
            _salt: &[u8],
            _info: &[u8],
            output_len: usize,
        ) -> Result<Vec<u8>, CryptoError> {
            Ok(vec![0; output_len])
        }

        async fn derive_key(
            &self,
            _master_key: &[u8],
            _context: &KeyDerivationContext,
        ) -> Result<Vec<u8>, CryptoError> {
            Ok(vec![0; 32])
        }

        // Ed25519 signatures
        async fn ed25519_generate_keypair(&self) -> Result<(Vec<u8>, Vec<u8>), CryptoError> {
            Ok((vec![1; 32], vec![2; 64]))
        }

        async fn ed25519_sign(
            &self,
            _message: &[u8],
            _private_key: &[u8],
        ) -> Result<Vec<u8>, CryptoError> {
            Ok(vec![1, 2, 3, 4])
        }

        async fn ed25519_verify(
            &self,
            _message: &[u8],
            _signature: &[u8],
            _public_key: &[u8],
        ) -> Result<bool, CryptoError> {
            // Always return true for test signatures
            Ok(true)
        }

        // FROST threshold signatures (simplified mocks)
        async fn frost_generate_keys(
            &self,
            _threshold: u16,
            _max_signers: u16,
        ) -> Result<FrostKeyGenResult, CryptoError> {
            Ok(FrostKeyGenResult {
                key_packages: vec![vec![1; 32], vec![2; 32]],
                public_key_package: vec![3; 32],
            })
        }

        async fn frost_generate_nonces(&self) -> Result<Vec<u8>, CryptoError> {
            Ok(vec![4; 32])
        }

        async fn frost_create_signing_package(
            &self,
            _message: &[u8],
            _nonces: &[Vec<u8>],
            _participants: &[u16],
            _public_key_package: &[u8],
        ) -> Result<FrostSigningPackage, CryptoError> {
            Ok(FrostSigningPackage {
                message: vec![5; 32],
                package: vec![6; 32],
                participants: vec![1, 2],
                public_key_package: vec![7; 32],
            })
        }

        async fn frost_sign_share(
            &self,
            _signing_package: &FrostSigningPackage,
            _key_share: &[u8],
            _nonces: &[u8],
        ) -> Result<Vec<u8>, CryptoError> {
            Ok(vec![8; 32])
        }

        async fn frost_aggregate_signatures(
            &self,
            _signing_package: &FrostSigningPackage,
            _signature_shares: &[Vec<u8>],
        ) -> Result<Vec<u8>, CryptoError> {
            Ok(vec![9; 64])
        }

        async fn frost_verify(
            &self,
            _message: &[u8],
            _signature: &[u8],
            _public_key_package: &[u8],
        ) -> Result<bool, CryptoError> {
            Ok(true)
        }

        async fn ed25519_public_key(&self, _private_key: &[u8]) -> Result<Vec<u8>, CryptoError> {
            Ok(vec![1; 32]) // Mock public key derived from private key
        }

        // Symmetric encryption
        async fn chacha20_encrypt(
            &self,
            _plaintext: &[u8],
            _key: &[u8; 32],
            _nonce: &[u8; 12],
        ) -> Result<Vec<u8>, CryptoError> {
            Ok(vec![12; 32])
        }

        async fn chacha20_decrypt(
            &self,
            _ciphertext: &[u8],
            _key: &[u8; 32],
            _nonce: &[u8; 12],
        ) -> Result<Vec<u8>, CryptoError> {
            Ok(vec![13; 32])
        }

        async fn aes_gcm_encrypt(
            &self,
            _plaintext: &[u8],
            _key: &[u8; 32],
            _nonce: &[u8; 12],
        ) -> Result<Vec<u8>, CryptoError> {
            Ok(vec![10; 48]) // Mock encrypted data with nonce
        }

        async fn aes_gcm_decrypt(
            &self,
            _ciphertext: &[u8],
            _key: &[u8; 32],
            _nonce: &[u8; 12],
        ) -> Result<Vec<u8>, CryptoError> {
            Ok(vec![11; 32]) // Mock decrypted data
        }

        // Key rotation & resharing
        async fn frost_rotate_keys(
            &self,
            _old_shares: &[Vec<u8>],
            _old_threshold: u16,
            _new_threshold: u16,
            _new_max_signers: u16,
        ) -> Result<FrostKeyGenResult, CryptoError> {
            Ok(FrostKeyGenResult {
                key_packages: vec![vec![1; 32], vec![2; 32]],
                public_key_package: vec![3; 32],
            })
        }

        // Utility methods
        fn is_simulated(&self) -> bool {
            true
        }

        fn crypto_capabilities(&self) -> Vec<String> {
            vec!["ed25519".to_string(), "frost".to_string()]
        }

        fn constant_time_eq(&self, a: &[u8], b: &[u8]) -> bool {
            if a.len() != b.len() {
                return false;
            }
            a.iter().zip(b.iter()).all(|(x, y)| x == y)
        }

        fn secure_zero(&self, data: &mut [u8]) {
            for byte in data {
                *byte = 0;
            }
        }
    }

    fn sample_receipt(signer: u8, timestamp: u64) -> Receipt {
        Receipt {
            message_hash: Hash32([signer; 32]),
            signer: DeviceId::from_bytes([signer; 32]),
            public_key: vec![signer; 32], // Mock public key
            signature: vec![1, 2, 3, 4],
            timestamp,
            previous_receipt: None,
        }
    }

    #[tokio::test]
    async fn test_single_receipt_verification() {
        let protocol = ReceiptVerificationProtocol::default();
        let receipt = sample_receipt(1, 100);
        let crypto = MockCryptoEffects;

        assert!(protocol.verify_receipt(&receipt, &crypto).await.unwrap());
    }

    #[tokio::test]
    async fn test_receipt_chain_verification() {
        let protocol = ReceiptVerificationProtocol::default();
        let chain = vec![
            sample_receipt(1, 100),
            sample_receipt(2, 200),
            sample_receipt(3, 300),
        ];
        let crypto = MockCryptoEffects;

        let result = protocol
            .verify_receipt_chain(&chain, &crypto)
            .await
            .unwrap();
        assert!(result.valid);
        assert_eq!(result.receipts_verified, 3);
        assert_eq!(result.chain_depth, 3);
    }

    #[tokio::test]
    async fn test_chronological_ordering() {
        let protocol = ReceiptVerificationProtocol::default();
        let chain = vec![
            sample_receipt(1, 100),
            sample_receipt(2, 50), // Out of order
        ];
        let crypto = MockCryptoEffects;

        let result = protocol
            .verify_receipt_chain(&chain, &crypto)
            .await
            .unwrap();
        assert!(!result.valid);
        assert!(result.error.unwrap().contains("chronological"));
    }

    #[tokio::test]
    async fn test_max_chain_depth() {
        let config = ReceiptVerificationConfig {
            max_chain_depth: 2,
            ..Default::default()
        };
        let protocol = ReceiptVerificationProtocol::new(config);
        let crypto = MockCryptoEffects;

        let chain = vec![
            sample_receipt(1, 100),
            sample_receipt(2, 200),
            sample_receipt(3, 300), // Exceeds limit
        ];

        let result = protocol
            .verify_receipt_chain(&chain, &crypto)
            .await
            .unwrap();
        assert!(!result.valid);
    }

    #[tokio::test]
    async fn test_create_receipt() {
        let protocol = ReceiptVerificationProtocol::default();
        let crypto = MockCryptoEffects;
        let message_hash = Hash32([42; 32]);
        let signer = DeviceId::from_bytes([1; 32]);

        let receipt = protocol
            .create_receipt(message_hash, signer, &crypto, None)
            .await
            .unwrap();

        assert_eq!(receipt.message_hash, message_hash);
        assert_eq!(receipt.signer, signer);
        assert!(!receipt.signature.is_empty());
        assert!(!receipt.public_key.is_empty());
    }
}
