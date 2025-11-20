//! Authority abstraction for the Aura platform
//!
//! This module provides the core Authority trait and related types for the
//! authority-centric architecture. Authorities are opaque cryptographic actors
//! that can sign operations and hold state without exposing internal device structure.

use crate::{identifiers::AuthorityId, Hash32, Result};
use async_trait::async_trait;
use std::sync::Arc;

// Type aliases for authority operations
type PublicKey = ed25519_dalek::VerifyingKey;
type Signature = ed25519_dalek::Signature;

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
    /// threshold structure, derived from the current ratchet tree state.
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
}

/// Type alias for shared authority references
pub type AuthorityRef = Arc<dyn Authority>;

/// Authority state representing the reduced view of an authority
///
/// This is computed deterministically from the authority's journal facts.
#[derive(Debug, Clone)]
pub struct AuthorityState {
    /// Current ratchet tree state (internal structure)
    pub tree_state: TreeState,
    /// Placeholder for journal facts - will be replaced with actual fact types
    pub facts: std::collections::BTreeSet<String>, // TODO: Replace with Fact type
}

/// Placeholder for ratchet tree state
///
/// TODO: This will be replaced with the actual ratchet tree implementation
/// from aura-journal when integrating with existing code.
#[derive(Debug, Clone)]
pub struct TreeState {
    /// Current commitment hash
    commitment: Hash32,
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
            commitment: Hash32::new([0; 32]),
        }
    }

    /// Get the root public key for the current tree state
    pub fn root_key(&self) -> PublicKey {
        // TODO: Implement actual derivation from tree
        // For now, derive a deterministic key from commitment
        let bytes = self.commitment.as_bytes();
        let mut key_bytes = [0u8; 32];
        key_bytes[..std::cmp::min(32, bytes.len())].copy_from_slice(&bytes[..std::cmp::min(32, bytes.len())]);
        PublicKey::from_bytes(&key_bytes).unwrap_or_else(|_| PublicKey::from_bytes(&[0; 32]).unwrap())
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
