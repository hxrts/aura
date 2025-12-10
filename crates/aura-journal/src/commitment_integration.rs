//! Integration between commitment tree operations and fact-based journal
//!
//! This module bridges the commitment tree implementation with the fact-based journal model,
//! providing conversion between AuthorityTreeState and aura-core TreeStateSummary.

use crate::commitment_tree::authority_state::AuthorityTreeState;
use crate::fact::AttestedOp;
use aura_core::authority::TreeStateSummary;
use aura_core::{epochs::Epoch, Hash32};

/// Convert AuthorityTreeState to the summary TreeStateSummary
impl From<&AuthorityTreeState> for TreeStateSummary {
    fn from(authority_state: &AuthorityTreeState) -> Self {
        TreeStateSummary::with_values(
            Epoch(authority_state.epoch),
            Hash32::new(authority_state.root_commitment),
            authority_state.get_threshold(),
            authority_state.active_leaf_count() as u32,
        )
    }
}

/// Extension trait for AuthorityTreeState conversions
pub trait TreeStateConversion {
    /// Convert to TreeStateSummary
    fn to_tree_state_summary(&self) -> TreeStateSummary;
}

impl TreeStateConversion for AuthorityTreeState {
    fn to_tree_state_summary(&self) -> TreeStateSummary {
        self.into()
    }
}

/// Convert AttestedOp to a format suitable for tree application
impl AttestedOp {
    /// Validate that this operation can be applied to the given parent state
    pub fn validate_against_parent(&self, parent_state: &TreeStateSummary) -> bool {
        self.parent_commitment == parent_state.root_commitment()
    }

    /// Get the resulting epoch after this operation
    pub fn resulting_epoch(&self, parent_epoch: Epoch) -> Epoch {
        // Tree operations increment the epoch
        Epoch(parent_epoch.0 + 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commitment_tree::authority_state::AuthorityTreeState;
    use crate::fact::TreeOpKind;

    #[test]
    fn test_tree_state_conversion() {
        let mut auth_state = AuthorityTreeState::new();
        auth_state.epoch = 5;
        auth_state.root_commitment = [1; 32];

        let tree_state = auth_state.to_tree_state_summary();

        assert_eq!(tree_state.epoch(), Epoch(5));

        // Debug the commitment issue
        let expected = [1; 32];
        let actual = tree_state.root_commitment().0;
        if actual != expected {
            println!("Expected: {:?}", expected);
            println!("Actual: {:?}", actual);
        }
        assert_eq!(actual, expected);

        assert_eq!(tree_state.threshold(), 1);
        assert_eq!(tree_state.device_count(), 0);
    }

    #[test]
    fn test_attested_op_validation() {
        let parent_commitment = Hash32::new([2; 32]);
        let tree_state = TreeStateSummary::with_values(Epoch(10), parent_commitment, 2, 3);

        let valid_op = AttestedOp {
            tree_op: TreeOpKind::AddLeaf {
                public_key: vec![0; 32],
                role: aura_core::tree::LeafRole::Device,
            },
            parent_commitment,
            new_commitment: Hash32::new([3; 32]),
            witness_threshold: 2,
            signature: vec![],
        };

        assert!(valid_op.validate_against_parent(&tree_state));

        let invalid_op = AttestedOp {
            parent_commitment: Hash32::from_bytes(&[99; 32]), // Wrong parent
            ..valid_op.clone()
        };

        assert!(!invalid_op.validate_against_parent(&tree_state));
    }
}
