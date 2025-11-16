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

use aura_core::{DeviceId, Hash32};
use crate::core::{SyncError, SyncResult};

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

    /// Verify a single receipt
    pub fn verify_receipt(&self, receipt: &Receipt) -> SyncResult<bool> {
        // TODO: Implement cryptographic verification
        // For now, basic validation
        if receipt.signature.is_empty() {
            return Err(SyncError::Verification(
                "Receipt has empty signature".to_string()
            ));
        }

        Ok(true)
    }

    /// Verify a receipt chain
    pub fn verify_receipt_chain(&self, receipts: &[Receipt]) -> SyncResult<VerificationResult> {
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
            // Verify individual receipt
            if !self.verify_receipt(receipt)? {
                return Ok(VerificationResult {
                    valid: false,
                    receipts_verified: signers.len(),
                    chain_depth: receipts.len(),
                    signers,
                    error: Some("Receipt verification failed".to_string()),
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

    fn sample_receipt(signer: u8, timestamp: u64) -> Receipt {
        Receipt {
            message_hash: Hash32([0; 32]),
            signer: DeviceId::from_bytes([signer; 32]),
            signature: vec![1, 2, 3],
            timestamp,
            previous_receipt: None,
        }
    }

    #[test]
    fn test_single_receipt_verification() {
        let protocol = ReceiptVerificationProtocol::default();
        let receipt = sample_receipt(1, 100);

        assert!(protocol.verify_receipt(&receipt).unwrap());
    }

    #[test]
    fn test_receipt_chain_verification() {
        let protocol = ReceiptVerificationProtocol::default();
        let chain = vec![
            sample_receipt(1, 100),
            sample_receipt(2, 200),
            sample_receipt(3, 300),
        ];

        let result = protocol.verify_receipt_chain(&chain).unwrap();
        assert!(result.valid);
        assert_eq!(result.receipts_verified, 3);
        assert_eq!(result.chain_depth, 3);
    }

    #[test]
    fn test_chronological_ordering() {
        let protocol = ReceiptVerificationProtocol::default();
        let chain = vec![
            sample_receipt(1, 100),
            sample_receipt(2, 50), // Out of order
        ];

        let result = protocol.verify_receipt_chain(&chain).unwrap();
        assert!(!result.valid);
        assert!(result.error.unwrap().contains("chronological"));
    }

    #[test]
    fn test_max_chain_depth() {
        let config = ReceiptVerificationConfig {
            max_chain_depth: 2,
            ..Default::default()
        };
        let protocol = ReceiptVerificationProtocol::new(config);

        let chain = vec![
            sample_receipt(1, 100),
            sample_receipt(2, 200),
            sample_receipt(3, 300), // Exceeds limit
        ];

        let result = protocol.verify_receipt_chain(&chain).unwrap();
        assert!(!result.valid);
    }
}
