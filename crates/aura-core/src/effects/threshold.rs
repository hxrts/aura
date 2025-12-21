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

use crate::threshold::{SigningContext, ThresholdConfig, ThresholdSignature, ThresholdState};
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

    /// Get the full threshold state for an authority
    ///
    /// Returns the complete threshold state including epoch number and guardian IDs.
    /// This is used by the recovery system to understand the current guardian
    /// configuration for prestate computation.
    ///
    /// Returns `None` if this service doesn't have signing capability for
    /// the authority.
    async fn threshold_state(&self, authority: &AuthorityId) -> Option<ThresholdState>;

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

    /// Rotate threshold keys to a new configuration
    ///
    /// Generates new threshold keys with the specified configuration and stores them
    /// at the next epoch. This is used for:
    /// - Guardian setup/change (creating new threshold group)
    /// - Upgrading from single-signer to threshold
    /// - Changing the k-of-n configuration
    ///
    /// The old keys are preserved at the previous epoch for potential rollback.
    /// Call `commit_key_rotation` after ceremony completion or `rollback_key_rotation`
    /// if the ceremony fails.
    ///
    /// # Arguments
    /// * `authority` - The authority to rotate keys for
    /// * `new_threshold` - New minimum signers required (k)
    /// * `new_total_participants` - New total number of key shares (n)
    /// * `guardian_ids` - IDs of the guardians who will hold shares
    ///
    /// # Returns
    /// A tuple of (new_epoch, key_packages, public_key_package) where:
    /// - new_epoch: The epoch number for the new keys
    /// - key_packages: One serialized key package per guardian
    /// - public_key_package: The group public key for verification
    async fn rotate_keys(
        &self,
        authority: &AuthorityId,
        new_threshold: u16,
        new_total_participants: u16,
        guardian_ids: &[String],
    ) -> Result<(u64, Vec<Vec<u8>>, PublicKeyPackage), ThresholdSigningError>;

    /// Commit a pending key rotation
    ///
    /// Called after a successful guardian ceremony when all guardians have accepted
    /// and stored their key shares. This makes the new epoch authoritative and
    /// allows the old epoch's keys to be eventually cleaned up.
    ///
    /// # Arguments
    /// * `authority` - The authority that was rotated
    /// * `new_epoch` - The epoch that should become active
    async fn commit_key_rotation(
        &self,
        authority: &AuthorityId,
        new_epoch: u64,
    ) -> Result<(), ThresholdSigningError>;

    /// Rollback a pending key rotation
    ///
    /// Called when a guardian ceremony fails (guardian declined, user cancelled,
    /// or timeout). This removes the new epoch's keys and reverts to the previous
    /// configuration.
    ///
    /// # Arguments
    /// * `authority` - The authority to rollback
    /// * `failed_epoch` - The epoch that should be discarded
    async fn rollback_key_rotation(
        &self,
        authority: &AuthorityId,
        failed_epoch: u64,
    ) -> Result<(), ThresholdSigningError>;
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
