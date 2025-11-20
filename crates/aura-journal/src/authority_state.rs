//! Authority state derivation from journal facts
//!
//! This module provides the implementation for deriving authority state
//! from the fact-based journal, implementing the Authority trait.

use crate::{
    fact::{Fact, FactContent},
    fact_journal::Journal,
    ratchet_tree::authority_state::AuthorityTreeState,
    reduction::reduce_authority,
};
use async_trait::async_trait;
use aura_core::{AuraError, Authority, AuthorityId, Hash32, Result};
use ed25519_dalek::{Signature, VerifyingKey as PublicKey};
use std::sync::Arc;

/// Authority state derived from facts
#[derive(Debug, Clone)]
pub struct AuthorityState {
    /// Tree state derived from attested operations
    pub tree_state: AuthorityTreeState,

    /// Threshold signing context (placeholder)
    pub threshold_context: Option<Vec<u8>>,
}

impl AuthorityState {
    /// Sign with threshold - internal implementation
    pub async fn sign_with_threshold(&self, data: &[u8]) -> Result<Signature> {
        // Get the root public key for signing context
        let public_key_bytes = self.tree_state.root_public_key()
            .ok_or_else(|| AuraError::Internal {
                message: "No public key available for threshold signing".to_string(),
            })?;

        // Convert to ed25519 public key for validation context
        let public_key_array: [u8; 32] = public_key_bytes.try_into()
            .map_err(|_| AuraError::Internal {
                message: "Invalid public key length for threshold signing".to_string(),
            })?;
            
        let _public_key = ed25519_dalek::VerifyingKey::from_bytes(&public_key_array)
            .map_err(|e| AuraError::Internal {
                message: format!("Invalid ed25519 public key: {}", e),
            })?;

        // TODO: Implement actual FROST threshold signing coordination
        // This would:
        // 1. Coordinate nonce generation across threshold devices
        // 2. Distribute partial signatures
        // 3. Aggregate into final signature
        // For now, create a deterministic placeholder signature for testing
        
        use ed25519_dalek::Signer;
        let signing_key = ed25519_dalek::SigningKey::from_bytes(&[42u8; 32]);
        let signature = signing_key.sign(data);
        
        Ok(signature)
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
}

#[async_trait]
impl Authority for DerivedAuthority {
    fn authority_id(&self) -> AuthorityId {
        self.id
    }

    fn public_key(&self) -> PublicKey {
        // Get root public key from tree state
        let key_bytes = self
            .state
            .tree_state
            .root_public_key()
            .unwrap_or_else(|| vec![0; 32]);

        // Convert to PublicKey (ed25519_dalek::VerifyingKey)
        let array: [u8; 32] = key_bytes.try_into().unwrap_or_else(|_| [0; 32]);

        PublicKey::from_bytes(&array).unwrap_or_else(|_| PublicKey::from_bytes(&[0; 32]).unwrap())
    }

    fn root_commitment(&self) -> Hash32 {
        Hash32::new(self.state.tree_state.root_commitment)
    }

    async fn sign_operation(&self, operation: &[u8]) -> Result<Signature> {
        // Delegate to internal threshold signing
        self.state.sign_with_threshold(operation).await
    }
}

/// Reduce journal facts to authority state
fn reduce_authority_state(_authority_id: AuthorityId, journal: &Journal) -> Result<AuthorityState> {
    // Start with empty tree state
    let mut tree_state = AuthorityTreeState::new();

    // Process facts in order
    // Note: The journal's namespace determines which authority this belongs to
    for fact in journal.iter_facts() {
        match &fact.content {
            FactContent::AttestedOp(op) => {
                // Apply tree operation
                apply_tree_op(&mut tree_state, op)?;
            }
            _ => {
                // Other fact types don't affect tree state
            }
        }
    }

    Ok(AuthorityState {
        tree_state,
        threshold_context: None,
    })
}

/// Apply an attested operation to the tree state
fn apply_tree_op(tree_state: &mut AuthorityTreeState, op: &crate::fact::AttestedOp) -> Result<()> {
    use crate::fact::TreeOpKind;

    match &op.tree_op {
        TreeOpKind::AddLeaf { public_key } => {
            // Add device without exposing device ID
            tree_state.add_device(public_key.clone());
        }
        TreeOpKind::RemoveLeaf { leaf_index } => {
            // Remove device by leaf index
            tree_state.remove_device(*leaf_index)?;
        }
        TreeOpKind::UpdatePolicy { threshold } => {
            // Update threshold policy
            tree_state.update_threshold(*threshold)?;
        }
        TreeOpKind::RotateEpoch => {
            // Rotate epoch and invalidate old shares
            tree_state.rotate_epoch()?;
        }
    }

    // Update commitment from the attested operation
    tree_state.root_commitment = op.new_commitment.0;

    Ok(())
}
