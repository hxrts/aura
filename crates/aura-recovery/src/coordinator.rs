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
use aura_core::identifiers::{AuthorityId, ContextId};
use aura_core::threshold::{SignableOperation, SigningContext, ThresholdSignature};
use aura_core::tree::TreeCommitment;
use std::sync::Arc;

/// Base trait for all recovery coordinators.
///
/// Coordinators are stateless - they derive state from facts in the journal.
/// Authorization is handled by the guard chain via choreography annotations.
#[async_trait]
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
            .map_err(|e| crate::RecoveryError::internal(format!("Guardian signing failed: {}", e)))
    }

    /// Create a success response with collected guardian signatures.
    ///
    /// Note: Each share contains an individual guardian's signature. The combined
    /// "aggregate" signature in the response is the first valid signature (for
    /// compatibility). True aggregation across authorities is not meaningful since
    /// each guardian signs with different keys.
    pub fn success_response(
        key_material: Option<Vec<u8>>,
        shares: Vec<RecoveryShare>,
        evidence: RecoveryEvidence,
    ) -> RecoveryResponse {
        // Use first valid signature as the "aggregate" for backward compatibility.
        // In practice, verification checks each guardian's individual signature
        // against their own public key.
        let signature = if let Some(first_share) = shares.first() {
            // Reconstruct a ThresholdSignature from the stored bytes
            ThresholdSignature::new(
                first_share.partial_signature.clone(),
                1,
                vec![0],
                Vec::new(), // public key would need to be looked up
                0,
            )
        } else {
            ThresholdSignature::new(vec![0u8; 64], 0, Vec::new(), Vec::new(), 0)
        };
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
        RecoveryResponse::success(key_material, shares, evidence, signature)
    }

    /// Create an error response.
    pub fn error_response(message: impl Into<String>) -> RecoveryResponse {
        RecoveryResponse::error(message)
    }

    /// Verify a guardian's signature over a recovery approval.
    ///
    /// Verifies that the signature was produced by the guardian's authority
    /// over the specified recovery operation.
    pub async fn verify_guardian_signature(
        &self,
        guardian_authority: &AuthorityId,
        target_authority: &AuthorityId,
        new_tree_root: &TreeCommitment,
        _session_id: &str,
        signature: &[u8],
    ) -> crate::RecoveryResult<bool> {
        use aura_core::effects::CryptoEffects;

        // Reconstruct the message that was signed
        let operation = SignableOperation::RecoveryApproval {
            target: *target_authority,
            new_root: *new_tree_root,
        };
        let message = serde_json::to_vec(&operation)
            .map_err(|e| crate::RecoveryError::internal(format!("Serialization failed: {}", e)))?;

        // Get the guardian's public key package
        let public_key = self
            .effect_system
            .public_key_package(guardian_authority)
            .await
            .ok_or_else(|| {
                crate::RecoveryError::not_found(format!(
                    "No public key for guardian: {:?}",
                    guardian_authority
                ))
            })?;

        // Verify the signature using CryptoEffects
        self.effect_system
            .frost_verify(&message, signature, &public_key)
            .await
            .map_err(|e| {
                crate::RecoveryError::internal(format!("Signature verification failed: {}", e))
            })
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
