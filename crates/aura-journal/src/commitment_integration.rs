//! Integration between commitment tree operations and fact-based journal
//!
//! This module bridges the commitment tree implementation with the fact-based journal model,
//! providing conversion between AuthorityTreeState and aura-core TreeStateSummary.

use crate::commitment_tree::authority_state::AuthorityTreeState;
use crate::fact::AttestedOp;
use aura_core::types::authority::TreeStateSummary;
use aura_core::types::Epoch;
use aura_core::Hash32;

/// Convert AuthorityTreeState to the summary TreeStateSummary
impl From<&AuthorityTreeState> for TreeStateSummary {
    fn from(authority_state: &AuthorityTreeState) -> Self {
        TreeStateSummary::with_values(
            authority_state.epoch,
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
    pub fn resulting_epoch(&self, parent_epoch: Epoch) -> Result<Epoch, aura_core::AuraError> {
        // Tree operations increment the epoch
        parent_epoch.next()
    }
}
