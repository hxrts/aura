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
    pub async fn sign_with_threshold(&self, _data: &[u8]) -> Result<Signature> {
        // TODO: Implement actual threshold signing
        // This would coordinate with local devices without exposing them
        Err(AuraError::Internal {
            message: "Threshold signing not yet implemented".to_string(),
        })
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
        TreeOpKind::RemoveLeaf { leaf_index: _leaf_index } => {
            // TODO: Implement leaf removal
            // tree_state.remove_device(leaf_index);
        }
        TreeOpKind::UpdatePolicy { threshold: _threshold } => {
            // TODO: Update threshold policy
            // tree_state.update_threshold(threshold);
        }
        TreeOpKind::RotateEpoch => {
            // TODO: Rotate epoch
            tree_state.epoch += 1;
        }
    }

    // Update commitment from the attested operation
    tree_state.root_commitment = op.new_commitment.0;

    Ok(())
}
