//! Authority state derivation from journal facts
//!
//! This module provides the implementation for deriving authority state
//! from the fact-based journal, implementing the Authority trait.
//!
//! # Threshold Signing
//!
//! Authority signing operations use `ThresholdSigningEffects` for all FROST
//! cryptographic operations. This provides:
//! - Unified signing across all scenarios (multi-device, recovery, groups)
//! - Proper key material management via secure storage
//! - Single-device fast path when threshold=1
//! - Multi-device coordination when threshold>1

use crate::{fact::Journal, reduction::reduce_account_facts};
use async_trait::async_trait;
use aura_core::effects::ThresholdSigningEffects;
use aura_core::threshold::{SignableOperation, SigningContext};
use aura_core::tree::TreeOp;
use aura_core::{authority::TreeStateSummary, AuraError, Authority, AuthorityId, Hash32, Result};
// Using aura-core type aliases for cryptographic types
use aura_core::authority::{Ed25519Signature as Signature, Ed25519VerifyingKey as PublicKey};

/// Authority state derived from facts
///
/// This struct represents the current state of an authority derived from
/// the fact-based journal. All signing operations use `ThresholdSigningEffects`
/// for proper FROST threshold cryptography.
#[derive(Debug, Clone)]
pub struct AuthorityState {
    /// Tree state summary derived from attested operations
    pub tree_state: TreeStateSummary,

    /// The authority identifier for signing operations
    pub authority_id: Option<AuthorityId>,
}

impl AuthorityState {
    /// Create a new authority state with just tree state summary
    pub fn new(tree_state: TreeStateSummary) -> Self {
        Self {
            tree_state,
            authority_id: None,
        }
    }

    /// Create authority state with authority ID for signing
    pub fn with_authority(tree_state: TreeStateSummary, authority_id: AuthorityId) -> Self {
        Self {
            tree_state,
            authority_id: Some(authority_id),
        }
    }

    /// Sign a tree operation using threshold signing effects.
    ///
    /// This delegates all FROST cryptographic operations to `ThresholdSigningEffects`,
    /// which handles:
    /// - Single-device fast path (no network for threshold=1)
    /// - Multi-device coordination (via choreography for threshold>1)
    /// - Key material retrieval from secure storage
    /// - Nonce generation, signing, and aggregation
    ///
    /// # Arguments
    /// - `effects`: The threshold signing effects implementation
    /// - `tree_op`: The tree operation to sign
    ///
    /// # Returns
    /// An Ed25519 signature over the serialized tree operation.
    pub async fn sign_tree_op<E: ThresholdSigningEffects>(
        &self,
        effects: &E,
        tree_op: TreeOp,
    ) -> Result<Signature> {
        let authority_id = self
            .authority_id
            .ok_or_else(|| AuraError::internal("Authority ID required for signing operations"))?;

        let signing_context = SigningContext::self_tree_op(authority_id, tree_op);

        tracing::debug!(
            ?authority_id,
            threshold = %self.tree_state.threshold(),
            devices = %self.tree_state.device_count(),
            "Signing tree operation via ThresholdSigningEffects"
        );

        let threshold_signature = effects
            .sign(signing_context)
            .await
            .map_err(|e| AuraError::internal(format!("Threshold signing failed: {e}")))?;

        // Convert ThresholdSignature to Ed25519 Signature
        let signature_bytes = threshold_signature.signature_bytes();
        if signature_bytes.len() != 64 {
            return Err(AuraError::invalid(format!(
                "Invalid signature length: {} (expected 64)",
                signature_bytes.len()
            )));
        }

        let mut sig_array = [0u8; 64];
        sig_array.copy_from_slice(signature_bytes);
        Ok(Signature::from_bytes(&sig_array))
    }

    /// Sign arbitrary message data using threshold signing effects.
    ///
    /// For signing arbitrary messages outside of tree operations, use the
    /// `Message` variant of `SignableOperation`.
    ///
    /// # Arguments
    /// - `effects`: The threshold signing effects implementation
    /// - `domain`: Domain separator for the message (e.g., "authority_state")
    /// - `data`: The message data to sign
    pub async fn sign_message<E: ThresholdSigningEffects>(
        &self,
        effects: &E,
        domain: &str,
        data: &[u8],
    ) -> Result<Signature> {
        let authority_id = self
            .authority_id
            .ok_or_else(|| AuraError::internal("Authority ID required for signing operations"))?;

        let signing_context = SigningContext {
            authority: authority_id,
            operation: SignableOperation::Message {
                domain: domain.to_string(),
                payload: data.to_vec(),
            },
            approval_context: aura_core::threshold::ApprovalContext::SelfOperation,
        };

        let threshold_signature = effects
            .sign(signing_context)
            .await
            .map_err(|e| AuraError::internal(format!("Threshold signing failed: {e}")))?;

        // Convert ThresholdSignature to Ed25519 Signature
        let signature_bytes = threshold_signature.signature_bytes();
        if signature_bytes.len() != 64 {
            return Err(AuraError::invalid(format!(
                "Invalid signature length: {} (expected 64)",
                signature_bytes.len()
            )));
        }

        let mut sig_array = [0u8; 64];
        sig_array.copy_from_slice(signature_bytes);
        Ok(Signature::from_bytes(&sig_array))
    }

}

/// Derived authority implementation
///
/// This implements the Authority trait by deriving all state from
/// the journal facts. No device information is exposed externally.
///
/// All signing operations use `ThresholdSigningEffects` for proper FROST
/// cryptographic operations.
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

    /// Sign a tree operation using threshold signing effects.
    ///
    /// This is the preferred method for signing tree operations.
    pub async fn sign_tree_operation<E: ThresholdSigningEffects>(
        &self,
        effects: &E,
        tree_op: TreeOp,
    ) -> Result<Signature> {
        self.state.sign_tree_op(effects, tree_op).await
    }

    /// Sign arbitrary message data using threshold signing effects.
    ///
    /// For signing messages outside of tree operations.
    pub async fn sign_message<E: ThresholdSigningEffects>(
        &self,
        effects: &E,
        domain: &str,
        data: &[u8],
    ) -> Result<Signature> {
        self.state.sign_message(effects, domain, data).await
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
            "Authority::sign_operation requires effect-backed crypto; use sign_tree_operation() or sign_message() instead",
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
    authority_id: AuthorityId,
    journal: &Journal,
) -> Result<AuthorityState> {
    // Use the reduction function to get tree state from facts
    let tree_state = reduce_account_facts(journal)
        .map_err(|e| AuraError::invalid(format!("Journal namespace mismatch: {e}")))?;

    Ok(AuthorityState::with_authority(tree_state, authority_id))
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
            assert_eq!(authority_state.authority_id, Some(authority_id));
            // The tree state should be initialized with default values
            assert_eq!(authority_state.tree_state.threshold(), 1); // Default threshold
        }
    }

    #[test]
    fn test_authority_state_creation() {
        // Test basic authority state creation
        let tree_state = aura_core::authority::TreeStateSummary::new();
        let authority_id = AuthorityId::new_from_entropy([9u8; 32]);

        let authority_state = AuthorityState::new(tree_state.clone());
        assert!(authority_state.authority_id.is_none());
        assert_eq!(authority_state.tree_state.threshold(), 1);

        let authority_state_with_id = AuthorityState::with_authority(tree_state, authority_id);
        assert_eq!(authority_state_with_id.authority_id, Some(authority_id));
        assert_eq!(authority_state_with_id.tree_state.threshold(), 1);
    }
}
