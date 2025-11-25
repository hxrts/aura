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
            .map_err(|e| sync_session_error(format!("Ed25519 verification failed: {}", e)))?;

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
    pub async fn create_receipt<E, T>(
        &self,
        message_hash: Hash32,
        signer: DeviceId,
        crypto_effects: &E,
        time_effects: &T,
        previous_receipt: Option<Box<Receipt>>,
    ) -> SyncResult<Receipt>
    where
        E: CryptoEffects + Send + Sync,
        T: aura_core::effects::PhysicalTimeEffects + Send + Sync,
    {
        // Get current timestamp from time provider (seconds precision for receipts)
        let timestamp = time_effects
            .physical_time()
            .await
            .map_err(|e| sync_session_error(format!("Failed to get timestamp: {}", e)))?
            .ts_ms
            / 1000;

        // Generate an Ed25519 key pair for signing (in production, this would retrieve the device's key)
        let (public_key, private_key) =
            crypto_effects
                .ed25519_generate_keypair()
                .await
                .map_err(|e| {
                    sync_session_error(format!("Failed to generate Ed25519 keypair: {}", e))
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
                sync_session_error(format!("Failed to create Ed25519 signature: {}", e))
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
    use aura_testkit::stateful_effects::{MockCryptoHandler, SimulatedTimeHandler};

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
        let config = ReceiptVerificationConfig {
            verify_signatures: false, // Disable signature verification for mock tests
            ..Default::default()
        };
        let protocol = ReceiptVerificationProtocol::new(config);
        let receipt = sample_receipt(1, 100);
        let crypto = MockCryptoHandler::new();

        assert!(protocol.verify_receipt(&receipt, &crypto).await.unwrap());
    }

    #[tokio::test]
    async fn test_receipt_chain_verification() {
        let config = ReceiptVerificationConfig {
            verify_signatures: false, // Disable signature verification for mock tests
            ..Default::default()
        };
        let protocol = ReceiptVerificationProtocol::new(config);
        let chain = vec![
            sample_receipt(1, 100),
            sample_receipt(2, 200),
            sample_receipt(3, 300),
        ];
        let crypto = MockCryptoHandler::new();

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
        let config = ReceiptVerificationConfig {
            verify_signatures: false, // Disable signature verification for mock tests
            ..Default::default()
        };
        let protocol = ReceiptVerificationProtocol::new(config);
        let chain = vec![
            sample_receipt(1, 100),
            sample_receipt(2, 50), // Out of order
        ];
        let crypto = MockCryptoHandler::new();

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
        let crypto = MockCryptoHandler::new();

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
        let crypto = MockCryptoHandler::new();
        let time = SimulatedTimeHandler::new();
        time.set_time(1_000);
        let message_hash = Hash32([42; 32]);
        let signer = DeviceId::from_bytes([1; 32]);

        let receipt = protocol
            .create_receipt(message_hash, signer, &crypto, &time, None)
            .await
            .unwrap();

        assert_eq!(receipt.message_hash, message_hash);
        assert_eq!(receipt.signer, signer);
        assert!(!receipt.signature.is_empty());
        assert!(!receipt.public_key.is_empty());
    }
}
