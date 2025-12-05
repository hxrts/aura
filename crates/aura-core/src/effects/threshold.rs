//! Threshold Signing Effects
//!
//! This module defines the trait interface for unified threshold signing operations.
//! The ThresholdSigningEffects trait provides a high-level API for threshold cryptographic
//! operations, abstracting the complexity of FROST key management and multi-party signing.
//!
//! # Effect Classification
//!
//! - **Category**: Application Effect
//! - **Implementation**: `aura-agent` (Layer 6) via `ThresholdSigningService`
//! - **Usage**: `aura-app`, `aura-terminal`, protocol crates needing threshold signatures
//!
//! # Unified Design
//!
//! This trait handles ALL threshold signing scenarios through context parameterization:
//! - Multi-device personal signing
//! - Guardian recovery approvals
//! - Group operation approvals
//! - Hybrid schemes (device + guardian)
//!
//! The same implementation handles all scenarios; only the `SigningContext` differs.

use crate::threshold::{SigningContext, ThresholdConfig, ThresholdSignature};
use crate::{AuraError, AuthorityId};
use async_trait::async_trait;

/// Threshold signing operation error
pub type ThresholdSigningError = AuraError;

/// Public key package bytes (serialized FROST group public key)
pub type PublicKeyPackage = Vec<u8>;

/// Threshold signing effects interface
///
/// Provides high-level threshold signing operations for all scenarios.
/// This is implemented by `ThresholdSigningService` in `aura-agent`.
///
/// # Unified API
///
/// The same methods are used for all threshold signing scenarios:
/// - Multi-device: Context has `ApprovalContext::SelfOperation`
/// - Guardian: Context has `ApprovalContext::RecoveryAssistance`
/// - Group: Context has `ApprovalContext::GroupDecision`
///
/// # Implementation Notes
///
/// Implementors must:
/// 1. Store key material in secure storage (via `SecureStorageEffects`)
/// 2. Handle single-signer fast path (no network for threshold=1)
/// 3. Coordinate multi-party signing (via choreography for threshold>1)
///
/// # Stability: UNSTABLE
/// This API is under active development and may change.
#[async_trait]
pub trait ThresholdSigningEffects: Send + Sync {
    /// Bootstrap a new authority with 1-of-1 keys
    ///
    /// Creates initial threshold keys for a new authority. This is used when:
    /// - Creating a new personal account
    /// - Creating a new group authority
    /// - Setting up a recovery authority
    ///
    /// The authority starts with threshold=1, which can be upgraded via DKG.
    ///
    /// # Returns
    /// The public key package for the authority, which should be stored
    /// in the commitment tree root.
    async fn bootstrap_authority(
        &self,
        authority: &AuthorityId,
    ) -> Result<PublicKeyPackage, ThresholdSigningError>;

    /// Sign an operation with threshold keys
    ///
    /// This unified signing method handles all scenarios:
    /// - Personal tree operations (multi-device)
    /// - Recovery approvals (guardian)
    /// - Group decisions (shared authority)
    ///
    /// # Single-Signer Fast Path
    ///
    /// When threshold=1, signing happens locally without network coordination.
    /// The same code path is used, just without the choreography.
    ///
    /// # Multi-Signer Coordination
    ///
    /// When threshold>1, the implementation coordinates with other participants
    /// via the ThresholdSign choreography.
    ///
    /// # Returns
    /// A `ThresholdSignature` containing the aggregate signature and metadata.
    async fn sign(
        &self,
        context: SigningContext,
    ) -> Result<ThresholdSignature, ThresholdSigningError>;

    /// Get the threshold configuration for an authority
    ///
    /// Returns the current m-of-n configuration for the authority's keys.
    /// Returns `None` if this service doesn't have signing capability for
    /// the authority.
    async fn threshold_config(&self, authority: &AuthorityId) -> Option<ThresholdConfig>;

    /// Check if this service can sign for an authority
    ///
    /// Returns `true` if this service holds a key share for the authority.
    /// This doesn't mean we can sign alone - threshold may require more signers.
    async fn has_signing_capability(&self, authority: &AuthorityId) -> bool;

    /// Get the public key package for an authority
    ///
    /// Returns the group public key if we have signing capability.
    /// This is needed for signature verification.
    async fn public_key_package(&self, authority: &AuthorityId) -> Option<PublicKeyPackage>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::threshold::{ApprovalContext, SignableOperation};
    use crate::tree::{TreeOp, TreeOpKind};

    fn test_authority() -> AuthorityId {
        AuthorityId::new_from_entropy([1u8; 32])
    }

    fn test_tree_op() -> TreeOp {
        TreeOp {
            parent_epoch: 0,
            parent_commitment: [0u8; 32],
            op: TreeOpKind::RotateEpoch { affected: vec![] },
            version: 1,
        }
    }

    #[test]
    fn test_signing_context_construction() {
        let context = SigningContext {
            authority: test_authority(),
            operation: SignableOperation::TreeOp(test_tree_op()),
            approval_context: ApprovalContext::SelfOperation,
        };

        assert!(matches!(
            context.approval_context,
            ApprovalContext::SelfOperation
        ));
    }
}
