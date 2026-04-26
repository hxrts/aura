//! Recovery coordinator infrastructure
//!
//! Provides stateless coordinator traits and base implementations for recovery operations.
//! All coordinators use the authority model and derive state from facts.
//!
//! # Guardian Signing Model
//!
//! Each guardian signs recovery approvals individually using their own authority's FROST keys.
//! The recovery threshold is about counting unique valid guardian signatures, not about
//! FROST threshold aggregation across guardians.
//!
//! ## Flow
//! 1. Guardian receives recovery request
//! 2. Guardian signs using `ThresholdSigningEffects::sign()` with `ApprovalContext::RecoveryAssistance`
//! 3. Recovery coordinator collects individual guardian signatures
//! 4. Threshold is met when enough guardians have submitted valid signatures

use crate::effects::RecoveryEffects;
use crate::types::{RecoveryEvidence, RecoveryResponse, RecoveryShare};
use crate::RecoveryResult;
use async_trait::async_trait;
use aura_core::threshold::{SigningContext, ThresholdSignature};
use aura_core::tree::TreeCommitment;
use aura_core::types::identifiers::{AuthorityId, ContextId};
use aura_core::TrustedKeyResolver;
use aura_signature::verify_threshold_signing_context_transcript;
use std::sync::Arc;

/// Base trait for all recovery coordinators.
///
/// Coordinators are stateless - they derive state from facts in the journal.
/// Authorization is handled by the guard chain via choreography annotations.
#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
pub trait RecoveryCoordinator<E: RecoveryEffects> {
    /// The request type for this coordinator
    type Request;
    /// The response type for this coordinator
    type Response;

    /// Get the effect system
    fn effect_system(&self) -> &Arc<E>;

    /// Execute the recovery operation
    async fn execute_recovery(&self, request: Self::Request) -> RecoveryResult<Self::Response>;

    /// Get the operation name (for logging and fact emission)
    fn operation_name(&self) -> &str;

    /// Generate a unique context ID for this operation
    fn generate_context_id(&self, account_id: &AuthorityId, discriminator: &str) -> ContextId {
        use aura_core::hash;
        let mut input = account_id.to_bytes().to_vec();
        input.extend_from_slice(discriminator.as_bytes());
        ContextId::new_from_entropy(hash::hash(&input))
    }
}

/// Stateless base coordinator.
///
/// Provides effect system access and common response builders.
/// Does not hold mutable state - all state is derived from facts.
pub struct BaseCoordinator<E: RecoveryEffects> {
    effect_system: Arc<E>,
}

impl<E: RecoveryEffects> BaseCoordinator<E> {
    /// Create a new base coordinator.
    pub fn new(effect_system: Arc<E>) -> Self {
        Self { effect_system }
    }

    /// Get the effect system.
    pub fn effect_system(&self) -> &Arc<E> {
        &self.effect_system
    }

    /// Create a guardian approval signature using threshold signing.
    ///
    /// Each guardian signs with their own authority's FROST keys. The signing context
    /// uses `ApprovalContext::RecoveryAssistance` which logs the recovery session
    /// for audit purposes.
    ///
    /// # Arguments
    /// - `guardian_authority`: The guardian's authority ID (their FROST keys)
    /// - `target_authority`: The authority being recovered
    /// - `new_tree_root`: The proposed new tree root after recovery
    /// - `session_id`: Unique recovery session identifier
    ///
    /// # Returns
    /// A threshold signature from the guardian's authority, or an error if signing fails.
    pub async fn sign_guardian_approval(
        &self,
        guardian_authority: AuthorityId,
        target_authority: AuthorityId,
        new_tree_root: TreeCommitment,
        session_id: String,
    ) -> crate::RecoveryResult<ThresholdSignature> {
        let signing_context = SigningContext::recovery_approval(
            guardian_authority,
            target_authority,
            new_tree_root,
            session_id,
        );

        self.effect_system
            .sign(signing_context)
            .await
            .map_err(|e| crate::RecoveryError::internal(format!("Guardian signing failed: {e}")))
    }

    /// Create a success response with collected guardian signatures.
    ///
    /// Each share contains an individual guardian signature. The top-level
    /// signature is only populated when an actual aggregate or explicit response
    /// signature exists in the evidence.
    pub fn success_response(
        key_material: Option<Vec<u8>>,
        shares: Vec<RecoveryShare>,
        evidence: RecoveryEvidence,
    ) -> RecoveryResponse {
        let signature = evidence.threshold_signature.clone();
        RecoveryResponse::success(key_material, shares, evidence, signature)
    }

    /// Create a success response with an explicit combined signature.
    ///
    /// Use this when you have collected threshold signatures from guardians
    /// and want to include them all in the evidence.
    pub fn success_response_with_signature(
        key_material: Option<Vec<u8>>,
        shares: Vec<RecoveryShare>,
        evidence: RecoveryEvidence,
        signature: ThresholdSignature,
    ) -> RecoveryResponse {
        RecoveryResponse::success(key_material, shares, evidence, Some(signature))
    }

    /// Create an error response.
    pub fn error_response(message: impl Into<String>) -> RecoveryResponse {
        RecoveryResponse::error(message)
    }

    /// Verify a guardian's signature over a recovery approval.
    ///
    /// Verifies that the signature was produced by the guardian's authority
    /// over the specified recovery operation using trusted authority/epoch key material.
    pub async fn verify_guardian_signature(
        &self,
        guardian_authority: &AuthorityId,
        guardian_epoch: u64,
        target_authority: &AuthorityId,
        new_tree_root: &TreeCommitment,
        session_id: &str,
        signature: &[u8],
        key_resolver: &impl TrustedKeyResolver,
    ) -> crate::RecoveryResult<bool> {
        let trusted_key = key_resolver
            .resolve_authority_threshold_key(*guardian_authority, guardian_epoch)
            .map_err(|e| {
                crate::RecoveryError::not_found(format!(
                    "No trusted threshold key for guardian {guardian_authority:?} at epoch {guardian_epoch}: {e}"
                ))
            })?;

        let signing_context = SigningContext::recovery_approval(
            *guardian_authority,
            *target_authority,
            *new_tree_root,
            session_id.to_string(),
        );

        verify_threshold_signing_context_transcript(
            self.effect_system.as_ref(),
            &signing_context,
            guardian_epoch,
            signature,
            trusted_key.bytes(),
        )
        .await
        .map_err(|e| crate::RecoveryError::internal(format!("Signature verification failed: {e}")))
    }
}

/// Helper trait for coordinators that use BaseCoordinator.
pub trait BaseCoordinatorAccess<E: RecoveryEffects> {
    /// Get access to the base coordinator
    fn base(&self) -> &BaseCoordinator<E>;

    /// Shortcut to effect system
    fn base_effect_system(&self) -> &Arc<E> {
        self.base().effect_system()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{RecoveryEvidence, RecoveryShare};

    #[test]
    fn success_response_does_not_fabricate_aggregate_signature() {
        let guardian = AuthorityId::new_from_entropy([7; 32]);
        let share = RecoveryShare {
            guardian_id: guardian,
            guardian_label: None,
            share: vec![1, 2, 3],
            partial_signature: vec![9; 64],
            issued_at_ms: 42,
        };
        let evidence = RecoveryEvidence {
            approving_guardians: vec![guardian],
            threshold_signature: None,
            ..RecoveryEvidence::default()
        };

        let response = BaseCoordinator::<aura_testkit::MockEffects>::success_response(
            None,
            vec![share],
            evidence,
        );

        assert!(response.signature.is_none());
    }
}
