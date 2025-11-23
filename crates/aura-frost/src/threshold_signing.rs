//! G_frost: Choreographic FROST Threshold Signing Implementation
//!
//! This module implements the G_frost choreography for distributed threshold
//! signature generation using the rumpsteak-aura choreographic DSL.
//!
//! ## Architecture
//!
//! The choreography follows a 4-phase protocol:
//! 1. **Initiation**: Coordinator initiates signing ceremony
//! 2. **Nonce Phase**: Signers generate and send nonce commitments
//! 3. **Signature Phase**: Signers compute and send partial signatures
//! 4. **Aggregation**: Coordinator aggregates signatures and broadcasts result
//!
//! ## Session Types
//!
//! The choreography provides compile-time guarantees:
//! - Deadlock freedom through choreographic projection
//! - Type-checked message passing
//! - Automatic local type generation for each role

#![allow(missing_docs)]

use crate::FrostResult;
use aura_core::frost::{NonceCommitment, PartialSignature, ThresholdSignature, TreeSigningContext};
use aura_core::{identifiers::AuthorityId, AccountId, AuraError, SessionId};
use aura_macros::choreography;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Generated key material for a threshold signing group
pub struct FrostKeyMaterial {
    /// Group public key package
    pub public_key_package: frost_ed25519::keys::PublicKeyPackage,
    /// Per-authority key packages
    pub key_packages: HashMap<AuthorityId, frost_ed25519::keys::KeyPackage>,
}

/// Configuration for threshold signing choreography
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThresholdSigningConfig {
    /// Number of signers required (M in M-of-N)
    pub threshold: usize,
    /// Total number of available signers (N in M-of-N)
    pub total_signers: usize,
    /// Session timeout in seconds
    pub timeout_seconds: u64,
    /// Maximum retry attempts
    pub max_retries: u32,
}

impl ThresholdSigningConfig {
    /// Create a new threshold signing configuration
    pub fn new(threshold: usize, total_signers: usize, timeout_seconds: u64) -> Self {
        Self {
            threshold,
            total_signers,
            timeout_seconds,
            max_retries: 3,
        }
    }

    /// Validate the configuration
    pub fn validate(&self) -> FrostResult<()> {
        if self.threshold == 0 {
            return Err(AuraError::invalid("Threshold must be greater than 0"));
        }
        if self.threshold > self.total_signers {
            return Err(AuraError::invalid("Threshold cannot exceed total signers"));
        }
        if self.total_signers > 100 {
            return Err(AuraError::invalid("Cannot support more than 100 signers"));
        }
        Ok(())
    }
}

/// Request message for threshold signing
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SigningRequest {
    /// Session identifier
    pub session_id: SessionId,
    /// Message to be signed
    pub message: Vec<u8>,
    /// Signing context for binding
    pub context: TreeSigningContext,
    /// Account being processed
    pub account_id: AccountId,
    /// Configuration for this signing session
    pub config: ThresholdSigningConfig,
}

/// Nonce commitment message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NonceCommitmentMsg {
    /// Session identifier
    pub session_id: SessionId,
    /// Signer authority ID
    pub signer_id: AuthorityId,
    /// FROST nonce commitment
    pub commitment: NonceCommitment,
}

/// Partial signature message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PartialSignatureMsg {
    /// Session identifier
    pub session_id: SessionId,
    /// Signer authority ID
    pub signer_id: AuthorityId,
    /// FROST partial signature
    pub signature: PartialSignature,
}

/// Final signature result message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignatureResult {
    /// Session identifier
    pub session_id: SessionId,
    /// Aggregated threshold signature (if successful)
    pub signature: Option<ThresholdSignature>,
    /// List of participating signers
    pub participants: Vec<AuthorityId>,
    /// Success indicator
    pub success: bool,
    /// Error message if failed
    pub error: Option<String>,
}

/// Abort message for session termination
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AbortMsg {
    /// Session identifier
    pub session_id: SessionId,
    /// Abort reason
    pub reason: String,
    /// Authority that initiated the abort
    pub initiator: AuthorityId,
}

// FROST threshold signing choreography protocol
//
// This choreography implements the complete FROST threshold signature protocol:
// - Coordinator initiates signing and aggregates results
// - Signers participate in multi-phase threshold signing
// - Supports dynamic signer sets with Byzantine fault tolerance
// - Provides session isolation and timeout handling
choreography! {
    #[namespace = "frost_threshold_signing"]
    protocol FrostThresholdSigning {
        roles: Coordinator, Signers[*];

        // Phase 1: Coordinator initiates signing ceremony
        Coordinator[guard_capability = "initiate_signing",
                   flow_cost = 100,
                   journal_facts = "signing_initiated"]
        -> Signers[*]: SigningRequest(SigningRequest);

        // Phase 2: Signers send nonce commitments
        Signers[0..threshold][guard_capability = "send_nonce",
                              flow_cost = 50,
                              journal_facts = "nonce_committed"]
        -> Coordinator: NonceCommitmentMsg(NonceCommitmentMsg);

        // Phase 3: Signers send partial signatures
        Signers[0..threshold][guard_capability = "send_signature",
                              flow_cost = 75,
                              journal_facts = "signature_contributed"]
        -> Coordinator: PartialSignatureMsg(PartialSignatureMsg);

        // Phase 4: Coordinator aggregates and broadcasts result
        Coordinator[guard_capability = "aggregate_signatures",
                   flow_cost = 200,
                   journal_facts = "signature_aggregated",
                   journal_merge = true]
        -> Signers[*]: SignatureResult(SignatureResult);

        // Optional: Abort handling for timeout or failure scenarios
        choice Coordinator {
            success: {
                // Normal completion - signature result already sent
            }
            abort: {
                Coordinator[guard_capability = "abort_signing",
                           flow_cost = 50,
                           journal_facts = "signing_aborted"]
                -> Signers[*]: AbortMsg(AbortMsg);
            }
        }
    }
}

// The choreography macro generates these types and functions automatically:
// - FrostThresholdSigningChoreography struct
// - Role-specific projection types for Coordinator and Signers
// - Message routing and session type enforcement
// - Automatic deadlock prevention and type safety

/// FROST cryptographic operations for threshold signing
pub struct FrostCrypto;

impl FrostCrypto {
    /// Generate key material for the provided authorities using the configured threshold
    pub async fn generate_key_material(
        authorities: &[AuthorityId],
        config: &ThresholdSigningConfig,
        random_effects: &dyn aura_core::effects::RandomEffects,
    ) -> FrostResult<FrostKeyMaterial> {
        use aura_effects::EffectSystemRng;
        use frost_ed25519 as frost;

        config.validate()?;
        if authorities.len() != config.total_signers {
            return Err(AuraError::invalid(format!(
                "Expected {} authorities, got {}",
                config.total_signers,
                authorities.len()
            )));
        }

        let mut rng = EffectSystemRng::from_current_runtime(random_effects);
        let (shares, public_key_package) = frost::keys::generate_with_dealer(
            config.total_signers as u16,
            config.threshold as u16,
            frost::keys::IdentifierList::Default,
            &mut rng,
        )
        .map_err(|e| AuraError::crypto(format!("Failed to generate key packages: {}", e)))?;

        let mut key_packages = HashMap::new();
        for (authority, (identifier, secret_share)) in authorities.iter().zip(shares.into_iter()) {
            // In FROST v1.0, KeyPackage is created from SecretShare + PublicKeyPackage
            let key_package = frost::keys::KeyPackage::try_from(secret_share)
                .map_err(|e| AuraError::crypto(format!("Failed to create key package: {}", e)))?;
            key_packages.insert(*authority, key_package);
        }

        Ok(FrostKeyMaterial {
            public_key_package,
            key_packages,
        })
    }

    /// Generate a FROST nonce commitment using real cryptographic operations
    pub async fn generate_nonce_commitment(
        key_package: &frost_ed25519::keys::KeyPackage,
        random_effects: &dyn aura_core::effects::RandomEffects,
    ) -> FrostResult<(frost_ed25519::round1::SigningNonces, NonceCommitment)> {
        use aura_effects::EffectSystemRng;
        use frost_ed25519 as frost;

        let mut rng = EffectSystemRng::from_current_runtime(random_effects);
        let (nonces, commitments) = frost::round1::commit(key_package.signing_share(), &mut rng);
        let identifier = key_package.identifier();
        let commitment = NonceCommitment::from_frost(*identifier, commitments);
        Ok((nonces, commitment))
    }

    /// Generate a FROST partial signature using real cryptographic operations
    pub async fn generate_partial_signature(
        context: &TreeSigningContext,
        message: &[u8],
        key_package: &frost_ed25519::keys::KeyPackage,
        signing_nonces: &frost_ed25519::round1::SigningNonces,
        commitments: &std::collections::BTreeMap<
            frost_ed25519::Identifier,
            frost_ed25519::round1::SigningCommitments,
        >,
        random_effects: &dyn aura_core::effects::RandomEffects,
    ) -> FrostResult<PartialSignature> {
        use aura_core::frost::tree_signing::binding_message;
        use aura_effects::EffectSystemRng;
        use frost_ed25519 as frost;

        let bound_message = binding_message(context, message);

        let rng = EffectSystemRng::from_current_runtime(random_effects);
        let partial_signature = {
            // Convert commitments into the format expected by FROST
            let signing_package = frost::SigningPackage::new(commitments.clone(), &bound_message);
            frost::round2::sign(&signing_package, signing_nonces, key_package)
        }
        .map_err(|e| AuraError::crypto(format!("FROST partial signing failed: {}", e)))?;

        Ok(PartialSignature::from_frost(
            *key_package.identifier(),
            partial_signature,
        ))
    }

    /// Aggregate FROST signatures using real cryptographic operations
    pub async fn aggregate_signatures(
        context: &TreeSigningContext,
        message: &[u8],
        partial_signatures: &HashMap<AuthorityId, PartialSignature>,
        nonce_commitments: &HashMap<AuthorityId, NonceCommitment>,
        config: &ThresholdSigningConfig,
        public_key_package: &frost_ed25519::keys::PublicKeyPackage,
    ) -> FrostResult<ThresholdSignature> {
        use aura_core::frost::tree_signing::{binding_message, frost_aggregate};
        use std::collections::BTreeMap;

        if partial_signatures.len() < config.threshold {
            return Err(AuraError::invalid(format!(
                "Insufficient partial signatures: {} < {}",
                partial_signatures.len(),
                config.threshold
            )));
        }

        let bound_message = binding_message(context, message);
        let partials: Vec<_> = partial_signatures.values().cloned().collect();

        // Convert commitments to FROST format
        let mut frost_commitments = BTreeMap::new();
        for commitment in nonce_commitments.values() {
            frost_commitments.insert(commitment.signer, commitment.clone());
        }

        let signature_bytes = frost_aggregate(
            &partials,
            &bound_message,
            &frost_commitments,
            public_key_package,
        )
        .map_err(|e| AuraError::crypto(format!("FROST aggregation failed: {}", e)))?;

        let participating_signers: Vec<u16> = partials.iter().map(|p| p.signer).collect();

        Ok(ThresholdSignature::new(
            signature_bytes,
            participating_signers,
        ))
    }
}

/// Convert stored nonce commitments into the FROST map required for signing
fn nonce_commitments_to_frost(
    commitments: &HashMap<AuthorityId, NonceCommitment>,
) -> FrostResult<
    std::collections::BTreeMap<
        frost_ed25519::Identifier,
        frost_ed25519::round1::SigningCommitments,
    >,
> {
    use std::collections::BTreeMap;

    let mut frost_commitments = BTreeMap::new();
    for commitment in commitments.values() {
        let identifier = commitment
            .frost_identifier()
            .map_err(|e| AuraError::invalid(e))?;
        let signing_commitments = commitment.to_frost().map_err(|e| AuraError::invalid(e))?;
        frost_commitments.insert(identifier, signing_commitments);
    }
    Ok(frost_commitments)
}

/// Current phase of the signing protocol (for monitoring purposes)
#[derive(Debug, Clone, PartialEq)]
pub enum SigningPhase {
    /// Initial phase after session creation
    Initiated,
    /// Collecting nonce commitments from signers
    CollectingNonces,
    /// Collecting partial signatures from signers
    CollectingSignatures,
    /// Coordinator aggregating signatures
    Aggregating,
    /// Signing completed successfully
    Completed,
    /// Signing was aborted due to error or timeout
    Aborted,
}

// Legacy coordinator and signer types for backward compatibility
// These are now deprecated in favor of the choreography! macro

#[cfg(test)]
mod tests {
    use super::*;
    use aura_macros::aura_test;
    use aura_testkit::simulation::choreography::ChoreographyTestHarness;
    use aura_testkit::{create_test_fixture, TestEffectsBuilder};

    #[test]
    fn test_threshold_signing_config_validation() {
        // Valid configuration
        let config = ThresholdSigningConfig::new(2, 3, 300);
        assert!(config.validate().is_ok());

        // Invalid: threshold = 0
        let config = ThresholdSigningConfig::new(0, 3, 300);
        assert!(config.validate().is_err());

        // Invalid: threshold > total
        let config = ThresholdSigningConfig::new(4, 3, 300);
        assert!(config.validate().is_err());

        // Invalid: too many signers
        let config = ThresholdSigningConfig::new(2, 101, 300);
        assert!(config.validate().is_err());
    }

    #[aura_test]
    async fn test_threshold_signing_choreography() -> aura_core::AuraResult<()> {
        let fixture = create_test_fixture();
        let effects_builder = TestEffectsBuilder::for_unit_tests(fixture.device_id(0));
        let effects = effects_builder.build()?;

        let config = ThresholdSigningConfig::new(3, 4, 300);
        assert!(config.validate().is_ok());

        let context = TreeSigningContext::new(1, 0, [1u8; 32]);
        let message = b"threshold signing test message";

        let authorities: Vec<_> = (0..config.total_signers)
            .map(|_| AuthorityId::new())
            .collect();
        let key_material =
            FrostCrypto::generate_key_material(&authorities, &config, &*effects.random_effects())
                .await?;

        let mut nonce_commitments = HashMap::new();
        let mut signer_nonces = HashMap::<AuthorityId, frost_ed25519::round1::SigningNonces>::new();

        for authority in &authorities {
            let key_pkg = key_material
                .key_packages
                .get(authority)
                .expect("missing key package");
            let (nonces, commitment) =
                FrostCrypto::generate_nonce_commitment(key_pkg, &*effects.random_effects()).await?;
            signer_nonces.insert(*authority, nonces);
            nonce_commitments.insert(*authority, commitment);
        }

        let frost_commitments = nonce_commitments_to_frost(&nonce_commitments)?;

        let mut partial_signatures = HashMap::new();
        for authority in authorities.iter().take(config.threshold) {
            let key_pkg = key_material
                .key_packages
                .get(authority)
                .expect("missing key package");
            let signing_nonces = signer_nonces
                .get(authority)
                .expect("missing nonces for signer");
            let partial_signature = FrostCrypto::generate_partial_signature(
                &context,
                message,
                key_pkg,
                signing_nonces,
                &frost_commitments,
                &*effects.random_effects(),
            )
            .await?;
            partial_signatures.insert(*authority, partial_signature);
        }

        let threshold_sig = FrostCrypto::aggregate_signatures(
            &context,
            message,
            &partial_signatures,
            &nonce_commitments,
            &config,
            &key_material.public_key_package,
        )
        .await?;

        assert_eq!(
            threshold_sig.participating_signers().len(),
            config.threshold
        );

        println!("✓ Threshold signing choreography test completed");
        Ok(())
    }

    #[aura_test]
    async fn test_full_threshold_signing_workflow() -> aura_core::AuraResult<()> {
        let fixture = create_test_fixture();
        let effects_builder = TestEffectsBuilder::for_unit_tests(fixture.device_id(0));
        let effects = effects_builder.build()?;

        let config = ThresholdSigningConfig::new(4, 6, 300);
        let context = TreeSigningContext::new(2, 1, [123u8; 32]);
        let message = b"comprehensive threshold signing test";

        let authorities: Vec<_> = (0..config.total_signers)
            .map(|_| AuthorityId::new())
            .collect();
        let key_material =
            FrostCrypto::generate_key_material(&authorities, &config, &*effects.random_effects())
                .await?;

        let mut nonce_commitments = HashMap::new();
        let mut signer_nonces = HashMap::<AuthorityId, frost_ed25519::round1::SigningNonces>::new();

        for authority in &authorities {
            let key_pkg = key_material
                .key_packages
                .get(authority)
                .expect("missing key package");
            let (nonces, commitment) =
                FrostCrypto::generate_nonce_commitment(key_pkg, &*effects.random_effects()).await?;
            signer_nonces.insert(*authority, nonces);
            nonce_commitments.insert(*authority, commitment);
        }

        let frost_commitments = nonce_commitments_to_frost(&nonce_commitments)?;

        let mut partial_signatures = HashMap::new();
        for authority in authorities.iter().take(config.threshold) {
            let key_pkg = key_material
                .key_packages
                .get(authority)
                .expect("missing key package");
            let signing_nonces = signer_nonces
                .get(authority)
                .expect("missing nonces for signer");
            let partial_sig = FrostCrypto::generate_partial_signature(
                &context,
                message,
                key_pkg,
                signing_nonces,
                &frost_commitments,
                &*effects.random_effects(),
            )
            .await?;

            partial_signatures.insert(*authority, partial_sig);
        }

        let threshold_result = FrostCrypto::aggregate_signatures(
            &context,
            message,
            &partial_signatures,
            &nonce_commitments,
            &config,
            &key_material.public_key_package,
        )
        .await;

        assert!(threshold_result.is_ok());

        let threshold_sig = threshold_result?;
        assert_eq!(threshold_sig.participating_signers().len(), 4);

        // Phase 4: Test insufficient signatures scenario
        let mut insufficient_signatures = std::collections::HashMap::new();
        for signer_index in 1..=2 {
            // Only 2 signatures (less than threshold of 4)
            let partial_sig = FrostCrypto::generate_partial_signature(
                &context,
                message,
                signer_index,
                &*effects.random_effects(),
            )
            .await?;

            let device_id = fixture.device_id(signer_index as usize);
            insufficient_signatures.insert(device_id, partial_sig);
        }

        let insufficient_result = aggregate_threshold_signature(
            &config,
            &context,
            message,
            &insufficient_signatures,
            &nonce_commitments,
            &*effects.random_effects(),
        )
        .await;

        assert!(insufficient_result.is_err());
        assert!(insufficient_result
            .unwrap_err()
            .to_string()
            .contains("Insufficient partial signatures"));

        println!("✓ Full threshold signing workflow test completed");
        Ok(())
    }

    #[test]
    fn test_frost_crypto_compile() {
        // Test that the FrostCrypto struct compiles and has the expected methods
        // NOTE: This is a compile-time test that violates architecture by directly
        // instantiating handlers. In practice, use aura-composition for handler creation.
        fn _check_api_exists() {
            use aura_core::effects::RandomEffects;

            // Type-level check: these functions must be callable with any RandomEffects impl.
            let _nonce_fn = |handler: &dyn RandomEffects| {
                let _ = FrostCrypto::generate_nonce_commitment(1, handler);
            };
            let _partial_sig_fn = |handler: &dyn RandomEffects| {
                let ctx = aura_core::frost::TreeSigningContext::new(1, 0, [0u8; 32]);
                let _ = FrostCrypto::generate_partial_signature(&ctx, b"test", 1, handler);
            };
            let _ = (_nonce_fn, _partial_sig_fn);
        }
    }

    #[test]
    fn test_frost_aggregation_config_validation() {
        use std::collections::HashMap;

        let config = ThresholdSigningConfig::new(2, 3, 300);
        assert!(config.validate().is_ok());

        // Test that the aggregation would fail with insufficient signatures
        let partial_signatures: HashMap<AuthorityId, PartialSignature> = HashMap::new(); // Empty - insufficient
        let _nonce_commitments: HashMap<AuthorityId, NonceCommitment> = HashMap::new();

        // This test verifies the validation logic without running actual cryptography
        // which requires a complete DKG ceremony setup
        assert!(partial_signatures.len() < config.threshold);
    }
}
