//! Recovery coordinator infrastructure
//!
//! Provides stateless coordinator traits and base implementations for recovery operations.
//! All coordinators use the authority model and derive state from facts.

use crate::effects::RecoveryEffects;
use crate::types::{RecoveryEvidence, RecoveryResponse, RecoveryShare};
use crate::RecoveryResult;
use async_trait::async_trait;
use aura_core::frost::ThresholdSignature;
use aura_core::identifiers::{AuthorityId, ContextId};
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

    /// Create a success response.
    pub fn success_response(
        key_material: Option<Vec<u8>>,
        shares: Vec<RecoveryShare>,
        evidence: RecoveryEvidence,
    ) -> RecoveryResponse {
        let signature = Self::aggregate_signatures(&shares);
        RecoveryResponse::success(key_material, shares, evidence, signature)
    }

    /// Create an error response.
    pub fn error_response(message: impl Into<String>) -> RecoveryResponse {
        RecoveryResponse::error(message)
    }

    /// Aggregate partial signatures from shares.
    pub fn aggregate_signatures(shares: &[RecoveryShare]) -> ThresholdSignature {
        if shares.is_empty() {
            return ThresholdSignature::new(vec![0u8; 64], Vec::new());
        }

        // Aggregate partial signatures
        let mut combined = Vec::new();
        for share in shares {
            combined.extend_from_slice(&share.partial_signature);
        }

        // Pad or truncate to 64 bytes
        let signature_bytes = if combined.len() >= 64 {
            combined[..64].to_vec()
        } else {
            combined.resize(64, 0);
            combined
        };

        // Use indices for signers
        let signers: Vec<u16> = (0..shares.len() as u16).collect();

        ThresholdSignature::new(signature_bytes, signers)
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
