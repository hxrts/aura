//! Authority abstraction for the Aura platform
//!
//! This module provides the core Authority trait and related types for the
//! authority-centric architecture. Authorities are opaque cryptographic actors
//! that can sign operations and hold state without exposing internal device structure.

use crate::{
    crypto::hash,
    domain::journal::Fact,
    tree::{policy::Policy, types::AttestedOp, types::TreeOpKind},
    types::identifiers::AuthorityId,
    types::sessions::Epoch,
    Hash32, Result,
};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, LazyLock};

// Public type aliases for authority operations
pub type Ed25519VerifyingKey = ed25519_dalek::VerifyingKey;
pub type Ed25519Signature = ed25519_dalek::Signature;
pub type Ed25519SigningKey = ed25519_dalek::SigningKey;

// Internal aliases for trait implementation
type PublicKey = Ed25519VerifyingKey;
type Signature = Ed25519Signature;
type SigningKey = Ed25519SigningKey;

/// Fallback public key for placeholder tree state
///
/// This is a known valid Ed25519 public key used as a fallback when
/// key derivation from commitment bytes fails. This is temporary code
/// until proper tree-based key derivation is implemented.
#[allow(clippy::incompatible_msrv)] // LazyLock is fine for this temporary fallback code
#[allow(clippy::expect_used)] // Hard-coded valid key - expect is safe here
#[allow(dead_code)] // Reserved for future use in authority validation
static FALLBACK_PUBLIC_KEY: LazyLock<PublicKey> = LazyLock::new(|| {
    // Using Ed25519 basepoint as a valid public key
    const VALID_PUBKEY_BYTES: [u8; 32] = [
        0x58, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66,
        0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66,
        0x66, 0x66,
    ];
    PublicKey::from_bytes(&VALID_PUBKEY_BYTES)
        .expect("Hard-coded valid Ed25519 public key should parse successfully")
});

/// Authority trait representing an opaque cryptographic actor
///
/// Authorities are the primary identity abstraction in Aura's new architecture.
/// They encapsulate internal device structure and threshold signing mechanisms,
/// exposing only the minimal interface needed for authentication and authorization.
#[async_trait]
pub trait Authority: Send + Sync {
    /// Get the unique identifier for this authority
    fn authority_id(&self) -> AuthorityId;

    /// Get the current public key for this authority
    ///
    /// This represents the root public key of the authority's internal
    /// threshold structure, derived from the current commitment tree state.
    fn public_key(&self) -> PublicKey;

    /// Get the current root commitment for this authority
    ///
    /// This is the hash of the authority's current reduced state,
    /// providing a compact representation for consensus operations.
    fn root_commitment(&self) -> Hash32;

    /// Sign an operation using the authority's threshold mechanism
    ///
    /// This triggers internal threshold signing without exposing
    /// which devices participated or the threshold structure.
    async fn sign_operation(&self, operation: &[u8]) -> Result<Signature>;

    /// Get the current threshold for this authority
    ///
    /// Returns the minimum number of devices required for threshold operations.
    fn get_threshold(&self) -> u16;

    /// Get the number of active devices in this authority
    ///
    /// Returns the count of currently active (non-removed) devices.
    fn active_device_count(&self) -> usize;
}

/// Type alias for shared authority references
pub type AuthorityRef = Arc<dyn Authority>;

/// Authority state representing the reduced view of an authority
///
/// This is computed deterministically from the authority's journal facts.
#[derive(Debug, Clone)]
pub struct AuthorityState {
    /// Current commitment tree state (internal structure)
    pub tree_state: TreeState,
    /// Journal facts that define this authority's state
    pub facts: std::collections::BTreeSet<Fact>,
}

/// Commitment tree state for authority management
///
/// This is the canonical tree state type used throughout the system.
/// It provides a public interface while hiding internal device structure.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TreeState {
    /// Current epoch
    epoch: Epoch,
    /// Current commitment hash
    commitment: Hash32,
    /// Threshold for operations
    threshold: u16,
    /// Number of active devices
    device_count: u32,
}

impl Default for TreeState {
    fn default() -> Self {
        Self::new()
    }
}

impl TreeState {
    /// Create a new tree state with zero commitment
    pub fn new() -> Self {
        Self {
            epoch: Epoch(0),
            commitment: Hash32::new([0; 32]),
            threshold: 1,
            device_count: 0,
        }
    }

    /// Create with specific values
    pub fn with_values(
        epoch: Epoch,
        commitment: Hash32,
        threshold: u16,
        device_count: u32,
    ) -> Self {
        Self {
            epoch,
            commitment,
            threshold,
            device_count,
        }
    }

    /// Get the current epoch
    pub fn epoch(&self) -> Epoch {
        self.epoch
    }

    /// Get the threshold
    pub fn threshold(&self) -> u16 {
        self.threshold
    }

    /// Get device count
    pub fn device_count(&self) -> u32 {
        self.device_count
    }

    /// Get the root public key for the current tree state
    pub fn root_key(&self) -> PublicKey {
        // Derive deterministic key material from the commitment to avoid ambient randomness.
        let seed = hash::hash(self.commitment.as_bytes());
        SigningKey::from_bytes(&seed).verifying_key()
    }

    /// Get the root commitment hash for the current tree state
    pub fn root_commitment(&self) -> Hash32 {
        self.commitment
    }

    /// Apply an attested operation to the tree state
    pub fn apply(&self, op: &AttestedOp) -> Self {
        let mut next = self.clone();

        // Advance epoch relative to parent binding
        next.epoch = Epoch::from(op.op.parent_epoch).next();

        // Update commitment deterministically from the operation payload
        if let Ok(bytes) = crate::util::serialization::to_vec(&op.op) {
            next.commitment = Hash32::from_bytes(&hash::hash(&bytes));
        }

        match &op.op.op {
            TreeOpKind::AddLeaf { .. } => {
                next.device_count = next.device_count.saturating_add(1);
            }
            TreeOpKind::RemoveLeaf { .. } => {
                next.device_count = next.device_count.saturating_sub(1);
            }
            TreeOpKind::ChangePolicy { new_policy, .. } => {
                next.threshold = match new_policy {
                    Policy::Any => 1,
                    Policy::Threshold { m, .. } => *m,
                    Policy::All => next.device_count.max(1).min(u16::MAX as u32) as u16,
                };
            }
            TreeOpKind::RotateEpoch { .. } => {
                // Epoch already advanced above; nothing else to do here.
            }
        }

        next
    }

    /// Update the commitment (internal method for reduction pipeline)
    pub fn _update_commitment(&mut self, new_commitment: Hash32) {
        self.commitment = new_commitment;
    }
}
