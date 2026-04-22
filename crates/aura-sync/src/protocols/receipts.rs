//! Receipt verification protocol
//!
//! Provides cryptographic receipt verification for multi-hop message chains
//! and attestation verification for distributed operations.
//!
//! # Usage
//!
//! ```rust,ignore
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
#[cfg(test)]
use aura_core::ContextId;
#[cfg(test)]
use aura_core::{hash, KeyResolutionError, TrustedKeyDomain, TrustedPublicKey};
use aura_core::{DeviceId, Hash32, TrustedKeyResolver};
use aura_guards::VerifiedIngress;
#[cfg(test)]
use aura_guards::{
    DecodedIngress, IngressSource, IngressVerificationEvidence, VerifiedIngressMetadata,
};
#[cfg(test)]
use aura_signature::sign_ed25519_transcript;
use aura_signature::{verify_ed25519_transcript, SecurityTranscript};

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

    /// Optional consensus instance id for finalized operations
    pub consensus_id: Option<Hash32>,

    /// Optional previous receipt in chain
    pub previous_receipt: Option<Box<Receipt>>,
}

#[derive(Debug, Clone, Serialize)]
struct ReceiptTranscriptPayload {
    message_hash: Hash32,
    signer: DeviceId,
    timestamp: u64,
    consensus_id: Option<Hash32>,
    previous_receipt_hash: Option<Hash32>,
}

struct ReceiptTranscript {
    message_hash: Hash32,
    signer: DeviceId,
    timestamp: u64,
    consensus_id: Option<Hash32>,
    previous_receipt_hash: Option<Hash32>,
}

impl SecurityTranscript for ReceiptTranscript {
    type Payload = ReceiptTranscriptPayload;

    const DOMAIN_SEPARATOR: &'static str = "aura.sync.receipt";

    fn transcript_payload(&self) -> Self::Payload {
        ReceiptTranscriptPayload {
            message_hash: self.message_hash,
            signer: self.signer,
            timestamp: self.timestamp,
            consensus_id: self.consensus_id,
            previous_receipt_hash: self.previous_receipt_hash,
        }
    }
}

fn receipt_transcript(
    message_hash: Hash32,
    signer: DeviceId,
    timestamp: u64,
    consensus_id: Option<Hash32>,
    previous_receipt: Option<&Receipt>,
) -> SyncResult<ReceiptTranscript> {
    let previous_receipt_hash = previous_receipt
        .map(Hash32::from_value)
        .transpose()
        .map_err(|error| sync_session_error(format!("Failed to hash previous receipt: {error}")))?;

    Ok(ReceiptTranscript {
        message_hash,
        signer,
        timestamp,
        consensus_id,
        previous_receipt_hash,
    })
}

/// Receipt chain verification result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationResult {
    /// Whether chain is valid
    pub valid: bool,

    /// Number of receipts verified
    pub receipts_verified: u32,

    /// Chain depth
    pub chain_depth: u32,

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
    pub max_chain_depth: u32,

    /// Require chronological ordering
    pub require_chronological: bool,

    /// Require consensus finalization evidence when verifying receipts
    pub require_consensus_finalization: bool,
}

impl Default for ReceiptVerificationConfig {
    fn default() -> Self {
        Self {
            max_chain_depth: 100,
            require_chronological: true,
            require_consensus_finalization: false,
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
    pub async fn verify_receipt<E>(
        &self,
        receipt: &VerifiedIngress<Receipt>,
        crypto_effects: &E,
        key_resolver: &impl TrustedKeyResolver,
    ) -> SyncResult<bool>
    where
        E: CryptoEffects + Send + Sync,
    {
        let expected_signer = receipt
            .evidence()
            .metadata()
            .source_device()
            .ok_or_else(|| sync_session_error("Receipt ingress source is not device-scoped"))?;
        self.verify_receipt_payload(
            receipt.payload(),
            expected_signer,
            crypto_effects,
            key_resolver,
        )
        .await
    }

    async fn verify_receipt_payload<E>(
        &self,
        receipt: &Receipt,
        expected_signer: DeviceId,
        crypto_effects: &E,
        key_resolver: &impl TrustedKeyResolver,
    ) -> SyncResult<bool>
    where
        E: CryptoEffects + Send + Sync,
    {
        // Basic validation
        if receipt.signature.is_empty() {
            return Err(sync_session_error("Receipt has empty signature"));
        }

        if receipt.signer != expected_signer {
            return Err(sync_session_error(
                "Receipt signer does not match verified ingress source",
            ));
        }

        if self.config.require_consensus_finalization && receipt.consensus_id.is_none() {
            return Err(sync_session_error(
                "Receipt missing consensus finalization evidence",
            ));
        }

        // Verify the signature using CryptoEffects
        tracing::debug!(
            "Verifying receipt signature for device {} over message hash {:?}",
            receipt.signer,
            receipt.message_hash
        );

        let trusted_key = key_resolver
            .resolve_device_key(expected_signer)
            .map_err(|e| {
                sync_session_error(format!("Receipt signer key resolution failed: {e}"))
            })?;

        let transcript = receipt_transcript(
            receipt.message_hash,
            receipt.signer,
            receipt.timestamp,
            receipt.consensus_id,
            receipt.previous_receipt.as_deref(),
        )?;

        // Use CryptoEffects to verify the signature with Ed25519
        let is_valid = verify_ed25519_transcript(
            crypto_effects,
            &transcript,
            &receipt.signature,
            trusted_key.bytes(),
        )
        .await
        .map_err(|e| sync_session_error(format!("Ed25519 verification failed: {e}")))?;

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
        receipts: &[VerifiedIngress<Receipt>],
        crypto_effects: &E,
        key_resolver: &impl TrustedKeyResolver,
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

        let receipts_len = receipts.len() as u32;

        if receipts_len > self.config.max_chain_depth {
            return Ok(VerificationResult {
                valid: false,
                receipts_verified: 0,
                chain_depth: receipts_len,
                signers: Vec::new(),
                error: Some("Chain exceeds maximum depth".to_string()),
            });
        }

        let mut signers = Vec::new();
        let mut last_timestamp = 0;

        for receipt in receipts {
            // Verify individual receipt cryptographically
            if !self
                .verify_receipt(receipt, crypto_effects, key_resolver)
                .await?
            {
                return Ok(VerificationResult {
                    valid: false,
                    receipts_verified: signers.len() as u32,
                    chain_depth: receipts_len,
                    signers,
                    error: Some("Receipt signature verification failed".to_string()),
                });
            }
            let receipt = receipt.payload();

            // Check chronological ordering
            if self.config.require_chronological && receipt.timestamp < last_timestamp {
                return Ok(VerificationResult {
                    valid: false,
                    receipts_verified: signers.len() as u32,
                    chain_depth: receipts_len,
                    signers,
                    error: Some("Receipts not in chronological order".to_string()),
                });
            }

            // Verify chain linkage if this receipt references a previous one
            if let Some(ref prev) = receipt.previous_receipt {
                if prev.timestamp > receipt.timestamp {
                    return Ok(VerificationResult {
                        valid: false,
                        receipts_verified: signers.len() as u32,
                        chain_depth: receipts_len,
                        signers,
                        error: Some("Previous receipt has later timestamp".to_string()),
                    });
                }

                // Verify the previous receipt too
                if !self
                    .verify_receipt_payload(prev, prev.signer, crypto_effects, key_resolver)
                    .await?
                {
                    return Ok(VerificationResult {
                        valid: false,
                        receipts_verified: signers.len() as u32,
                        chain_depth: receipts_len,
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
            receipts_verified: receipts_len,
            chain_depth: receipts_len,
            signers,
            error: None,
        })
    }

    /// Create a cryptographically signed receipt for tests.
    #[cfg(test)]
    pub async fn create_receipt_with_ephemeral_key_for_tests<E, T>(
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
            .map_err(|e| sync_session_error(format!("Failed to get timestamp: {e}")))?
            .ts_ms
            / 1000;

        // Generate an Ed25519 key pair for signing using injected CryptoEffects
        let (private_key, public_key) = crypto_effects
            .ed25519_generate_keypair()
            .await
            .map_err(|e| sync_session_error(format!("Failed to generate Ed25519 keypair: {e}")))?;

        let transcript = receipt_transcript(
            message_hash,
            signer,
            timestamp,
            None,
            previous_receipt.as_deref(),
        )?;

        // Create Ed25519 signature
        let signature = sign_ed25519_transcript(crypto_effects, &transcript, &private_key)
            .await
            .map_err(|e| sync_session_error(format!("Failed to create Ed25519 signature: {e}")))?;

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
            consensus_id: None,
            previous_receipt,
        })
    }
}

#[cfg(test)]
fn verified_receipt_for_tests(receipt: Receipt) -> VerifiedIngress<Receipt> {
    verified_receipt_from_source_for_tests(receipt.signer, receipt)
}

#[cfg(test)]
fn verified_receipt_from_source_for_tests(
    source: DeviceId,
    receipt: Receipt,
) -> VerifiedIngress<Receipt> {
    let metadata = VerifiedIngressMetadata::new(
        IngressSource::Device(source),
        ContextId::new_from_entropy(*receipt.message_hash.as_bytes()),
        None,
        receipt.message_hash,
        1,
    );
    let evidence = IngressVerificationEvidence::new(
        metadata.clone(),
        aura_guards::REQUIRED_INGRESS_VERIFICATION_CHECKS,
    )
    .unwrap();
    DecodedIngress::new(receipt, metadata)
        .verify(evidence)
        .unwrap()
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
    use aura_effects::RealCryptoHandler;
    use aura_testkit::stateful_effects::{MockCryptoHandler, SimulatedTimeHandler};
    use std::collections::BTreeMap;

    #[derive(Default)]
    struct TestKeyResolver {
        device_keys: BTreeMap<DeviceId, Vec<u8>>,
    }

    impl TestKeyResolver {
        fn with_device_key(mut self, device: DeviceId, key: Vec<u8>) -> Self {
            self.device_keys.insert(device, key);
            self
        }
    }

    impl TrustedKeyResolver for TestKeyResolver {
        fn resolve_authority_threshold_key(
            &self,
            _authority: aura_core::AuthorityId,
            _epoch: u64,
        ) -> Result<TrustedPublicKey, KeyResolutionError> {
            Err(KeyResolutionError::Unknown {
                domain: TrustedKeyDomain::AuthorityThreshold,
            })
        }

        fn resolve_device_key(
            &self,
            device: DeviceId,
        ) -> Result<TrustedPublicKey, KeyResolutionError> {
            let key = self
                .device_keys
                .get(&device)
                .ok_or(KeyResolutionError::Unknown {
                    domain: TrustedKeyDomain::Device,
                })?;
            Ok(TrustedPublicKey::active(
                TrustedKeyDomain::Device,
                None,
                key.clone(),
                Hash32::new(hash::hash(key)),
            ))
        }

        fn resolve_guardian_key(
            &self,
            _guardian: aura_core::AuthorityId,
        ) -> Result<TrustedPublicKey, KeyResolutionError> {
            Err(KeyResolutionError::Unknown {
                domain: TrustedKeyDomain::Guardian,
            })
        }

        fn resolve_release_key(
            &self,
            _authority: aura_core::AuthorityId,
        ) -> Result<TrustedPublicKey, KeyResolutionError> {
            Err(KeyResolutionError::Unknown {
                domain: TrustedKeyDomain::Release,
            })
        }
    }

    fn sample_receipt(signer: u8, timestamp: u64) -> Receipt {
        Receipt {
            message_hash: Hash32([signer; 32]),
            signer: DeviceId::from_bytes([signer; 32]),
            public_key: vec![signer; 32], // Mock public key
            signature: vec![1, 2, 3, 4],
            timestamp,
            consensus_id: None,
            previous_receipt: None,
        }
    }

    async fn signed_receipt_for_tests(
        protocol: &ReceiptVerificationProtocol,
        crypto: &RealCryptoHandler,
        time: &SimulatedTimeHandler,
        signer: u8,
        timestamp_ms: u64,
    ) -> Receipt {
        time.set_time(timestamp_ms);
        protocol
            .create_receipt_with_ephemeral_key_for_tests(
                Hash32([signer; 32]),
                DeviceId::from_bytes([signer; 32]),
                crypto,
                time,
                None,
            )
            .await
            .unwrap()
    }

    fn keys_for_receipts(receipts: &[Receipt]) -> TestKeyResolver {
        receipts
            .iter()
            .fold(TestKeyResolver::default(), |keys, receipt| {
                keys.with_device_key(receipt.signer, receipt.public_key.clone())
            })
    }

    #[aura_macros::aura_test]
    async fn test_single_receipt_verification() {
        let protocol = ReceiptVerificationProtocol::default();
        let crypto = RealCryptoHandler::for_simulation_seed([1; 32]);
        let time = SimulatedTimeHandler::new();
        let receipt = signed_receipt_for_tests(&protocol, &crypto, &time, 1, 100_000).await;
        let keys = keys_for_receipts(std::slice::from_ref(&receipt));

        assert!(protocol
            .verify_receipt(&verified_receipt_for_tests(receipt), &crypto, &keys)
            .await
            .unwrap());
    }

    #[aura_macros::aura_test]
    async fn receipt_signer_must_match_verified_ingress_source() {
        let protocol = ReceiptVerificationProtocol::default();
        let crypto = RealCryptoHandler::for_simulation_seed([2; 32]);
        let time = SimulatedTimeHandler::new();
        let receipt = signed_receipt_for_tests(&protocol, &crypto, &time, 1, 100_000).await;
        let verified =
            verified_receipt_from_source_for_tests(DeviceId::from_bytes([2; 32]), receipt);
        let keys = TestKeyResolver::default();

        let error = protocol
            .verify_receipt(&verified, &crypto, &keys)
            .await
            .expect_err("mismatched ingress signer should fail");
        assert!(error.to_string().contains("verified ingress source"));
    }

    #[aura_macros::aura_test]
    async fn test_receipt_chain_verification() {
        let protocol = ReceiptVerificationProtocol::default();
        let crypto = RealCryptoHandler::for_simulation_seed([3; 32]);
        let time = SimulatedTimeHandler::new();
        let receipts = vec![
            signed_receipt_for_tests(&protocol, &crypto, &time, 1, 100_000).await,
            signed_receipt_for_tests(&protocol, &crypto, &time, 2, 200_000).await,
            signed_receipt_for_tests(&protocol, &crypto, &time, 3, 300_000).await,
        ];
        let keys = keys_for_receipts(&receipts);
        let chain = receipts
            .into_iter()
            .map(verified_receipt_for_tests)
            .collect::<Vec<_>>();

        let result = protocol
            .verify_receipt_chain(&chain, &crypto, &keys)
            .await
            .unwrap();
        assert!(result.valid);
        assert_eq!(result.receipts_verified, 3);
        assert_eq!(result.chain_depth, 3);
    }

    #[aura_macros::aura_test]
    async fn test_chronological_ordering() {
        let protocol = ReceiptVerificationProtocol::default();
        let crypto = RealCryptoHandler::for_simulation_seed([4; 32]);
        let time = SimulatedTimeHandler::new();
        let receipts = vec![
            signed_receipt_for_tests(&protocol, &crypto, &time, 1, 100_000).await,
            signed_receipt_for_tests(&protocol, &crypto, &time, 2, 50_000).await, // Out of order
        ];
        let keys = keys_for_receipts(&receipts);
        let chain = receipts
            .into_iter()
            .map(verified_receipt_for_tests)
            .collect::<Vec<_>>();

        let result = protocol
            .verify_receipt_chain(&chain, &crypto, &keys)
            .await
            .unwrap();
        assert!(!result.valid);
        assert!(result.error.unwrap().contains("chronological"));
    }

    #[aura_macros::aura_test]
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
        ]
        .into_iter()
        .map(verified_receipt_for_tests)
        .collect::<Vec<_>>();
        let keys = TestKeyResolver::default();

        let result = protocol
            .verify_receipt_chain(&chain, &crypto, &keys)
            .await
            .unwrap();
        assert!(!result.valid);
    }

    #[aura_macros::aura_test]
    async fn test_create_receipt_with_ephemeral_key_for_tests() {
        let protocol = ReceiptVerificationProtocol::default();
        let crypto = MockCryptoHandler::new();
        let time = SimulatedTimeHandler::new();
        time.set_time(1_000);
        let message_hash = Hash32([42; 32]);
        let signer = DeviceId::from_bytes([1; 32]);

        let receipt = protocol
            .create_receipt_with_ephemeral_key_for_tests(message_hash, signer, &crypto, &time, None)
            .await
            .unwrap();

        assert_eq!(receipt.message_hash, message_hash);
        assert_eq!(receipt.signer, signer);
        assert!(!receipt.signature.is_empty());
        assert!(!receipt.public_key.is_empty());
    }

    #[aura_macros::aura_test]
    async fn receipt_verification_uses_resolved_device_key_not_embedded_key() {
        let protocol = ReceiptVerificationProtocol::default();
        let crypto = RealCryptoHandler::for_simulation_seed([9; 32]);
        let time = SimulatedTimeHandler::new();
        time.set_time(1_000);
        let signer = DeviceId::from_bytes([9; 32]);
        let mut receipt = protocol
            .create_receipt_with_ephemeral_key_for_tests(
                Hash32([7; 32]),
                signer,
                &crypto,
                &time,
                None,
            )
            .await
            .unwrap();
        let trusted_public_key = receipt.public_key.clone();

        receipt.public_key = vec![0xAA; 32];
        let keys = TestKeyResolver::default().with_device_key(signer, trusted_public_key.clone());

        assert!(protocol
            .verify_receipt(&verified_receipt_for_tests(receipt.clone()), &crypto, &keys)
            .await
            .unwrap());

        let wrong_keys = TestKeyResolver::default().with_device_key(signer, vec![0xBB; 32]);
        assert!(!protocol
            .verify_receipt(&verified_receipt_for_tests(receipt), &crypto, &wrong_keys)
            .await
            .unwrap());
    }

    #[test]
    fn receipt_transcript_binds_previous_receipt() {
        let current = sample_receipt(2, 200);
        let previous = sample_receipt(1, 100);

        let without_previous = receipt_transcript(
            current.message_hash,
            current.signer,
            current.timestamp,
            current.consensus_id,
            None,
        )
        .unwrap()
        .transcript_bytes()
        .unwrap();
        let with_previous = receipt_transcript(
            current.message_hash,
            current.signer,
            current.timestamp,
            current.consensus_id,
            Some(&previous),
        )
        .unwrap()
        .transcript_bytes()
        .unwrap();

        assert_ne!(without_previous, with_previous);
    }
}
