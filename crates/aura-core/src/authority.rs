//! Authority abstraction for the Aura platform
//!
//! This module provides the core Authority trait and related types for the
//! authority-centric architecture. Authorities are opaque cryptographic actors
//! that can sign operations and hold state without exposing internal device structure.

use crate::{identifiers::AuthorityId, journal::Fact, session_epochs::Epoch, Hash32, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, LazyLock};

// Type aliases for authority operations
type PublicKey = ed25519_dalek::VerifyingKey;
type Signature = ed25519_dalek::Signature;

/// Fallback public key for placeholder tree state
///
/// This is a known valid Ed25519 public key used as a fallback when
/// key derivation from commitment bytes fails. This is temporary code
/// until proper tree-based key derivation is implemented.
#[allow(clippy::incompatible_msrv)] // LazyLock is fine for this temporary fallback code
#[allow(clippy::expect_used)] // Hard-coded valid key - expect is safe here
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
        // TODO: Implement actual derivation from tree
        // For now, derive a deterministic key from commitment
        let bytes = self.commitment.as_bytes();
        let mut key_bytes = [0u8; 32];
        key_bytes[..std::cmp::min(32, bytes.len())]
            .copy_from_slice(&bytes[..std::cmp::min(32, bytes.len())]);
        PublicKey::from_bytes(&key_bytes).unwrap_or(*FALLBACK_PUBLIC_KEY)
    }

    /// Get the root commitment hash for the current tree state
    pub fn root_commitment(&self) -> Hash32 {
        self.commitment
    }

    /// Apply an attested operation to the tree state
    pub fn apply(&self, _op: &[u8]) -> Self {
        // TODO: Implement actual tree update logic with AttestedOp
        self.clone()
    }

    /// Update the commitment (internal method for reduction pipeline)
    pub fn _update_commitment(&mut self, new_commitment: Hash32) {
        self.commitment = new_commitment;
    }
}
