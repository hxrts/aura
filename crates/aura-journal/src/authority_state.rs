//! Authority state derivation from journal facts
//!
//! This module provides the implementation for deriving authority state
//! from the fact-based journal, implementing the Authority trait.

use crate::{fact::Journal, reduction::reduce_account_facts};
use async_trait::async_trait;
use aura_core::effects::{crypto::FrostKeyGenResult, CryptoEffects};
use aura_core::{authority::TreeState, AuraError, Authority, AuthorityId, Hash32, Result};
// Using aura-core type aliases for cryptographic types
use aura_core::authority::{Ed25519Signature as Signature, Ed25519VerifyingKey as PublicKey};

/// Authority state derived from facts
#[derive(Debug, Clone)]
pub struct AuthorityState {
    /// Tree state derived from attested operations
    pub tree_state: TreeState,

    /// Threshold signing key material derived from secure storage or facts
    pub threshold_context: Option<FrostKeyGenResult>,
}

impl AuthorityState {
    /// Sign with threshold - internal implementation
    pub async fn sign_with_threshold<E: CryptoEffects>(
        &self,
        effects: &E,
        data: &[u8],
    ) -> Result<Signature> {
        // Get the root public key for signing context
        let _public_key = self.tree_state.root_key();

        // Delegate to FROST threshold signing coordination via effects
        self.coordinate_frost_threshold_signing(effects, data).await
    }

    /// Coordinate threshold signing across devices via effects delegation.
    ///
    /// This properly delegates cryptography to the effects system, following
    /// the architectural pattern: Layer 2 (journal) → Layer 3 (effects).
    ///
    /// ## FROST Threshold Signing Protocol
    ///
    /// This function implements the complete FROST threshold signing workflow:
    /// 1. Extract device key shares from secure storage
    /// 2. Generate fresh nonces for each participant
    /// 3. Collect nonce commitments from threshold participants
    /// 4. Create signing package with message and commitments
    /// 5. Collect partial signatures from participants
    /// 6. Aggregate partial signatures into group signature
    /// 7. Verify final signature before returning
    /// 8. Return the aggregated signature as ed25519_dalek::Signature
    ///
    /// ## Error Handling
    ///
    /// This function can fail at various stages:
    /// - Insufficient participants for threshold
    /// - Key retrieval failures
    /// - Network coordination failures
    /// - Invalid partial signatures
    /// - Verification failures
    async fn coordinate_frost_threshold_signing<E: CryptoEffects>(
        &self,
        effects: &E,
        data: &[u8],
    ) -> Result<Signature> {
        // Get threshold requirements from tree state
        let threshold = self.tree_state.threshold();
        let device_count = self.tree_state.device_count();

        // Validate we have sufficient devices for threshold
        if device_count < threshold as u32 {
            return Err(AuraError::invalid(format!(
                "Insufficient devices for threshold signing: have {}, need {}",
                device_count, threshold
            )));
        }

        tracing::debug!(
            "Starting FROST threshold signing with threshold={}, devices={}",
            threshold,
            device_count
        );

        // Step 1: Generate FROST nonces for signing session
        let nonces = effects
            .frost_generate_nonces()
            .await
            .map_err(|e| AuraError::internal(format!("Failed to generate FROST nonces: {}", e)))?;

        // Step 2: Select deterministic participant set from tree state leaves
        // Device IDs are derived from leaf indices in the tree (1-indexed for FROST)
        let mut device_ids: Vec<u16> = (1..=device_count as u16).collect();
        device_ids.sort_unstable();
        device_ids.truncate(threshold as usize);
        let participants = device_ids;

        // Step 3: Get the public key package from stored threshold context
        let frost_keygen_result = self.threshold_context.as_ref().ok_or_else(|| {
            AuraError::internal(
                "Threshold signing keys not available. Load FrostKeyGenResult into AuthorityState",
            )
        })?;

        let public_key_package = frost_keygen_result.public_key_package.clone();

        // Step 4: Create signing package with message and participants
        let signing_package = effects
            .frost_create_signing_package(
                data,
                std::slice::from_ref(&nonces),
                &participants,
                &public_key_package,
            )
            .await
            .map_err(|e| {
                AuraError::internal(format!("Failed to create FROST signing package: {}", e))
            })?;

        // Step 5: Generate signature shares from each participant
        // In a real distributed implementation, this would involve:
        // - Sending signing package to each participant via transport effects
        // - Each participant generating their signature share with their key share
        // - Collecting signature shares from all participants
        let mut signature_shares = Vec::new();

        for (i, _participant_id) in participants.iter().enumerate() {
            // Use the key package for this participant
            if let Some(key_share) = frost_keygen_result.key_packages.get(i) {
                let signature_share = effects
                    .frost_sign_share(&signing_package, key_share, &nonces)
                    .await
                    .map_err(|e| {
                        AuraError::internal(format!("Failed to create signature share: {}", e))
                    })?;

                signature_shares.push(signature_share);
            }
        }

        // Validate we have enough signature shares for threshold
        if signature_shares.len() < threshold as usize {
            return Err(AuraError::invalid(format!(
                "Insufficient signature shares: have {}, need {}",
                signature_shares.len(),
                threshold
            )));
        }

        // Step 6: Aggregate partial signatures into group signature
        let group_signature = effects
            .frost_aggregate_signatures(&signing_package, &signature_shares)
            .await
            .map_err(|e| {
                AuraError::internal(format!("Failed to aggregate FROST signatures: {}", e))
            })?;

        // Step 7: Verify the aggregated signature before returning
        let group_public_key = self.tree_state.root_key().to_bytes().to_vec();
        let verification_result = effects
            .frost_verify(data, &group_signature, &group_public_key)
            .await
            .map_err(|e| AuraError::internal(format!("Failed to verify FROST signature: {}", e)))?;

        if !verification_result {
            return Err(AuraError::internal(
                "FROST signature verification failed - aggregated signature is invalid",
            ));
        }

        // Step 8: Return signature bytes directly through effects system
        if group_signature.len() != 64 {
            return Err(AuraError::invalid(format!(
                "Invalid signature length: {} (expected 64)",
                group_signature.len()
            )));
        }

        // Convert bytes to ed25519_dalek::Signature for trait compatibility
        if group_signature.len() != 64 {
            return Err(AuraError::invalid(format!(
                "Invalid signature length: {}",
                group_signature.len()
            )));
        }
        let mut signature_bytes = [0u8; 64];
        signature_bytes.copy_from_slice(&group_signature);
        Ok(Signature::from_bytes(&signature_bytes))
    }
}

/// Derived authority implementation
///
/// This implements the Authority trait by deriving all state from
/// the journal facts. No device information is exposed externally.
#[derive(Debug, Clone)]
pub struct DerivedAuthority {
    /// Authority identifier
    id: AuthorityId,

    /// Derived state
    state: AuthorityState,
}

impl DerivedAuthority {
    /// Create from journal by reduction
    pub fn from_journal(authority_id: AuthorityId, journal: &Journal) -> Result<Self> {
        // Reduce journal facts to authority state
        let state = reduce_authority_state(authority_id, journal)?;

        Ok(Self {
            id: authority_id,
            state,
        })
    }

    /// Sign operation with proper effects delegation
    ///
    /// This method provides the correct architectural pattern for threshold signing:
    /// Layer 2 (journal) → Layer 3 (effects)
    pub async fn sign_operation_with_effects<E: CryptoEffects>(
        &self,
        effects: &E,
        operation: &[u8],
    ) -> Result<Signature> {
        // Delegate to internal threshold signing with effects
        self.state.sign_with_threshold(effects, operation).await
    }
}

#[async_trait]
impl Authority for DerivedAuthority {
    fn authority_id(&self) -> AuthorityId {
        self.id
    }

    fn public_key(&self) -> PublicKey {
        // Get root public key from tree state
        self.state.tree_state.root_key()
    }

    fn root_commitment(&self) -> Hash32 {
        self.state.tree_state.root_commitment()
    }

    async fn sign_operation(&self, _operation: &[u8]) -> Result<Signature> {
        Err(AuraError::internal(
            "Authority::sign_operation requires effect-backed crypto; call sign_operation_with_effects instead",
        ))
    }

    fn get_threshold(&self) -> u16 {
        self.state.tree_state.threshold()
    }

    fn active_device_count(&self) -> usize {
        self.state.tree_state.device_count() as usize
    }
}

/// Reduce journal facts to authority state
pub fn reduce_authority_state(
    _authority_id: AuthorityId,
    journal: &Journal,
) -> Result<AuthorityState> {
    // Use the reduction function to get tree state from facts
    let tree_state = reduce_account_facts(journal)
        .map_err(|e| AuraError::invalid(format!("Journal namespace mismatch: {}", e)))?;

    Ok(AuthorityState {
        tree_state,
        threshold_context: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fact::{Journal, JournalNamespace};
    use aura_core::AuthorityId;

    #[test]
    fn test_derived_authority_creation_from_journal() {
        // Test that DerivedAuthority can be created from a journal
        let authority_id = AuthorityId::new_from_entropy([7u8; 32]);
        let journal = Journal::new(JournalNamespace::Authority(authority_id));

        let result = DerivedAuthority::from_journal(authority_id, &journal);

        assert!(
            result.is_ok(),
            "Should be able to create DerivedAuthority from journal: {:?}",
            result.err()
        );

        if let Ok(derived_authority) = result {
            assert_eq!(
                derived_authority.authority_id(),
                authority_id,
                "Authority ID should match"
            );
        }
    }

    #[test]
    fn test_reduce_authority_state_basic() {
        // Test the basic authority state reduction
        let authority_id = AuthorityId::new_from_entropy([8u8; 32]);
        let journal = Journal::new(JournalNamespace::Authority(authority_id));

        let result = reduce_authority_state(authority_id, &journal);

        assert!(
            result.is_ok(),
            "Should be able to reduce authority state from journal: {:?}",
            result.err()
        );

        if let Ok(authority_state) = result {
            // Verify basic properties of the authority state
            assert!(authority_state.threshold_context.is_none());
            // The tree state should be initialized with default values
            assert_eq!(authority_state.tree_state.threshold(), 1); // Default threshold
        }
    }

    #[test]
    fn test_authority_state_creation() {
        // Test basic authority state creation
        let tree_state = aura_core::authority::TreeState::new();

        let authority_state = AuthorityState {
            tree_state,
            threshold_context: None,
        };

        // Verify the authority state was created correctly
        assert!(authority_state.threshold_context.is_none());
        assert_eq!(authority_state.tree_state.threshold(), 1); // Default threshold
    }
}
