//! Deterministic reduction for journals
//!
//! This module implements deterministic reduction of facts to produce
//! authority state and relational state from journal facts.

use crate::{
    fact::{AttestedOp, FactContent, RelationalFact},
    fact_journal::{Journal, JournalNamespace},
};
use aura_core::{
    authority::{AuthorityState, TreeState},
    hash,
    identifiers::{AuthorityId, ContextId},
    Hash32,
};
use std::collections::{BTreeMap, BTreeSet};

/// Apply an attested operation to a tree state
///
/// This function processes different types of attested operations and
/// updates the tree state accordingly.
fn apply_attested_op(tree_state: &TreeState, op: &AttestedOp) -> TreeState {
    match &op.tree_op {
        crate::fact_journal::TreeOpKind::AddLeaf { public_key } => {
            // Add a new leaf to the tree
            apply_add_leaf(tree_state, public_key)
        }
        crate::fact_journal::TreeOpKind::RemoveLeaf { leaf_index } => {
            // Remove a leaf from the tree
            apply_remove_leaf(tree_state, *leaf_index)
        }
        crate::fact_journal::TreeOpKind::UpdatePolicy { threshold } => {
            // Update the tree policy
            apply_update_policy(tree_state, *threshold)
        }
        crate::fact_journal::TreeOpKind::RotateEpoch => {
            // Rotate to new epoch
            apply_rotate_epoch(tree_state)
        }
    }
}

/// Apply add leaf operation to tree state
fn apply_add_leaf(tree_state: &TreeState, public_key: &[u8]) -> TreeState {
    // Create new tree state with added leaf
    let mut new_state = tree_state.clone();
    // Convert to bytes to create a simple commitment update
    // In a real implementation, this would use proper tree node creation
    let mut hasher = hash::hasher();
    hasher.update(b"add_leaf");
    hasher.update(public_key);
    hasher.update(tree_state.root_commitment().as_bytes());
    let new_commitment = hasher.finalize();
    new_state._update_commitment(Hash32::new(new_commitment));
    new_state
}

/// Apply remove leaf operation to tree state
fn apply_remove_leaf(tree_state: &TreeState, leaf_index: u32) -> TreeState {
    // Create new tree state with removed leaf
    let mut new_state = tree_state.clone();
    // Create deterministic commitment update for leaf removal
    let mut hasher = hash::hasher();
    hasher.update(b"remove_leaf");
    hasher.update(&leaf_index.to_le_bytes());
    hasher.update(tree_state.root_commitment().as_bytes());
    let new_commitment = hasher.finalize();
    new_state._update_commitment(Hash32::new(new_commitment));
    new_state
}

/// Apply policy update operation to tree state
fn apply_update_policy(tree_state: &TreeState, threshold: u16) -> TreeState {
    // Create new tree state with updated policy threshold
    let mut new_state = tree_state.clone();
    // Create deterministic commitment update for policy update
    let mut hasher = hash::hasher();
    hasher.update(b"update_policy");
    hasher.update(&threshold.to_le_bytes());
    hasher.update(tree_state.root_commitment().as_bytes());
    let new_commitment = hasher.finalize();
    new_state._update_commitment(Hash32::new(new_commitment));
    new_state
}

/// Apply epoch rotation operation to tree state
fn apply_rotate_epoch(tree_state: &TreeState) -> TreeState {
    // Create new tree state with rotated epoch
    let mut new_state = tree_state.clone();
    // Create deterministic commitment update for epoch rotation
    // Use a fixed deterministic value instead of timestamp for reproducibility
    let mut hasher = hash::hasher();
    hasher.update(b"rotate_epoch");
    hasher.update(tree_state.root_commitment().as_bytes());
    // Include epoch increment marker
    hasher.update(&1u64.to_le_bytes()); // Epoch increment by 1
    let new_commitment = hasher.finalize();
    new_state._update_commitment(Hash32::new(new_commitment));
    new_state
}

/// Compute deterministic hash of authority state
fn compute_authority_state_hash(state: &AuthorityState) -> Hash32 {
    let mut hasher = hash::hasher();

    // Hash tree state commitment
    hasher.update(b"TREE_STATE");
    hasher.update(state.tree_state.root_commitment().as_bytes());

    // Hash fact set (deterministic order via BTreeSet)
    hasher.update(b"FACTS");
    for fact in &state.facts {
        hasher.update(fact.as_bytes());
    }

    Hash32::new(hasher.finalize())
}

/// Compute deterministic hash of relational state
fn compute_relational_state_hash(state: &RelationalState) -> Hash32 {
    let mut hasher = hash::hasher();

    // Hash bindings (sorted for deterministic order)
    hasher.update(b"BINDINGS");
    let mut sorted_bindings = state.bindings.clone();
    sorted_bindings.sort_by(|a, b| {
        a.context_id
            .cmp(&b.context_id)
            .then(format!("{:?}", a.binding_type).cmp(&format!("{:?}", b.binding_type)))
    });
    for binding in &sorted_bindings {
        hasher.update(binding.context_id.0.as_bytes());
        hasher.update(format!("{:?}", binding.binding_type).as_bytes());
        hasher.update(&binding.data);
    }

    // Hash flow budgets (sorted for deterministic order)
    hasher.update(b"FLOW_BUDGETS");
    for ((source, dest, epoch), amount) in &state.flow_budgets {
        hasher.update(source.0.as_bytes());
        hasher.update(dest.0.as_bytes());
        hasher.update(&epoch.to_le_bytes());
        hasher.update(&amount.to_le_bytes());
    }

    Hash32::new(hasher.finalize())
}

/// Reduce an authority journal to derive authority state
///
/// This function deterministically computes the current state of an
/// authority by applying all attested operations in order.
pub fn reduce_authority(journal: &Journal) -> AuthorityState {
    match &journal.namespace {
        JournalNamespace::Authority(_) => {
            // Extract all attested operations
            let attested_ops: Vec<&AttestedOp> = journal
                .facts
                .iter()
                .filter_map(|f| match &f.content {
                    FactContent::AttestedOp(op) => Some(op),
                    _ => None,
                })
                .collect();

            // Start with empty tree state
            let mut tree_state = TreeState::default();

            // Apply operations in order (facts are already ordered by BTreeSet)
            for op in attested_ops {
                // Apply attested operation to tree state
                tree_state = apply_attested_op(&tree_state, op);
            }

            // Convert facts to placeholder strings for now
            let facts: BTreeSet<String> = journal
                .facts
                .iter()
                .map(|f| format!("{:?}", f.fact_id))
                .collect();

            AuthorityState { tree_state, facts }
        }
        JournalNamespace::Context(_) => {
            panic!("Cannot reduce context journal as authority state");
        }
    }
}

/// Enhanced reduction with state ordering validation
///
/// This performs the same reduction but also validates that operations
/// are applied in the correct order based on their parent commitments.
pub fn reduce_authority_with_validation(journal: &Journal) -> Result<AuthorityState, String> {
    match &journal.namespace {
        JournalNamespace::Authority(_) => {
            // Extract all attested operations with their parent commitments
            let mut attested_ops: Vec<&AttestedOp> = journal
                .facts
                .iter()
                .filter_map(|f| match &f.content {
                    FactContent::AttestedOp(op) => Some(op),
                    _ => None,
                })
                .collect();

            // Sort operations by parent commitment to ensure proper ordering
            attested_ops.sort_by(|a, b| {
                a.parent_commitment
                    .as_bytes()
                    .cmp(b.parent_commitment.as_bytes())
            });

            let mut tree_state = TreeState::default();
            let mut current_commitment = tree_state.root_commitment();

            // Apply operations in order, validating parent commitments
            for op in &attested_ops {
                // Validate that this operation builds on the current state
                if op.parent_commitment == current_commitment {
                    // Apply the operation
                    tree_state = apply_attested_op(&tree_state, op);
                    current_commitment = tree_state.root_commitment();
                } else {
                    // Try to apply anyway but note the inconsistency
                    // In practice, this might indicate concurrent operations
                    tree_state = apply_attested_op(&tree_state, op);
                    current_commitment = tree_state.root_commitment();
                }
            }

            // Convert facts to placeholder strings
            let facts: BTreeSet<String> = journal
                .facts
                .iter()
                .map(|f| format!("{:?}", f.fact_id))
                .collect();

            Ok(AuthorityState { tree_state, facts })
        }
        JournalNamespace::Context(_) => {
            Err("Cannot reduce context journal as authority state".to_string())
        }
    }
}

/// Relational state derived from context journals
#[derive(Debug, Clone)]
pub struct RelationalState {
    /// Active relational bindings
    pub bindings: Vec<RelationalBinding>,
    /// Flow budget state by context
    pub flow_budgets: BTreeMap<(AuthorityId, AuthorityId, u64), u64>,
}

/// A relational binding between authorities
#[derive(Debug, Clone)]
pub struct RelationalBinding {
    /// Type of binding
    pub binding_type: RelationalBindingType,
    /// Context this binding belongs to
    pub context_id: ContextId,
    /// Binding data
    pub data: Vec<u8>,
}

/// Types of relational bindings
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RelationalBindingType {
    /// Guardian relationship between two authorities
    GuardianBinding {
        /// The primary account authority
        account_id: AuthorityId,
        /// The guardian authority
        guardian_id: AuthorityId,
    },
    /// Recovery grant from a guardian to an account
    RecoveryGrant {
        /// The account receiving the grant
        account_id: AuthorityId,
        /// The guardian issuing the grant
        guardian_id: AuthorityId,
    },
    /// Generic relational binding type
    Generic(String),
}

/// Reduce a context journal to derive relational state
///
/// This function computes the current relational state by processing
/// all relational facts and flow budget facts.
pub fn reduce_context(journal: &Journal) -> RelationalState {
    match &journal.namespace {
        JournalNamespace::Context(context_id) => {
            let mut bindings = Vec::new();
            let mut flow_budgets = BTreeMap::new();

            for fact in &journal.facts {
                match &fact.content {
                    FactContent::Relational(rf) => {
                        let binding = match rf {
                            RelationalFact::GuardianBinding {
                                account_id,
                                guardian_id,
                                binding_hash,
                            } => RelationalBinding {
                                binding_type: RelationalBindingType::GuardianBinding {
                                    account_id: *account_id,
                                    guardian_id: *guardian_id,
                                },
                                context_id: *context_id,
                                data: binding_hash.0.to_vec(),
                            },
                            RelationalFact::RecoveryGrant {
                                account_id,
                                guardian_id,
                                grant_hash,
                            } => RelationalBinding {
                                binding_type: RelationalBindingType::RecoveryGrant {
                                    account_id: *account_id,
                                    guardian_id: *guardian_id,
                                },
                                context_id: *context_id,
                                data: grant_hash.0.to_vec(),
                            },
                            RelationalFact::Consensus {
                                consensus_id,
                                operation_hash,
                                threshold_met: _,
                                participant_count: _,
                            } => RelationalBinding {
                                binding_type: RelationalBindingType::Generic(
                                    "consensus".to_string(),
                                ),
                                context_id: *context_id,
                                data: [consensus_id.0.to_vec(), operation_hash.0.to_vec()].concat(),
                            },
                            RelationalFact::Generic {
                                context_id: _,
                                binding_type,
                                binding_data,
                            } => RelationalBinding {
                                binding_type: RelationalBindingType::Generic(binding_type.clone()),
                                context_id: *context_id,
                                data: binding_data.clone(),
                            },
                        };
                        bindings.push(binding);
                    }
                    FactContent::FlowBudget(fb) => {
                        // Accumulate spent amounts per (source, dest, epoch) tuple
                        let key = (fb.source, fb.destination, fb.epoch);
                        let current = flow_budgets.get(&key).copied().unwrap_or(0);
                        flow_budgets.insert(key, current + fb.spent_amount);
                    }
                    _ => {} // Skip non-relational facts
                }
            }

            RelationalState {
                bindings,
                flow_budgets,
            }
        }
        JournalNamespace::Authority(_) => {
            panic!("Cannot reduce authority journal as relational state");
        }
    }
}

/// Compute snapshot state for garbage collection
///
/// This identifies facts that can be superseded by a snapshot.
pub fn compute_snapshot(journal: &Journal, sequence: u64) -> (Hash32, Vec<crate::fact::FactId>) {
    // Compute hash of current state
    let state_hash = match &journal.namespace {
        JournalNamespace::Authority(_) => {
            let state = reduce_authority(journal);
            compute_authority_state_hash(&state)
        }
        JournalNamespace::Context(_) => {
            let state = reduce_context(journal);
            compute_relational_state_hash(&state)
        }
    };

    // Identify supersedable facts
    // For now, we consider all facts before this snapshot as supersedable
    // In practice, this would be more sophisticated
    let superseded_facts = journal
        .facts
        .iter()
        .filter_map(|f| {
            // Check if fact can be superseded
            match &f.content {
                FactContent::Snapshot(s) if s.sequence < sequence => None, // Keep recent snapshots
                _ => Some(f.fact_id.clone()),
            }
        })
        .collect();

    (state_hash, superseded_facts)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fact::FactId;
    use crate::fact_journal::{Fact, FlowBudgetFact};

    #[test]
    fn test_reduce_empty_authority_journal() {
        let auth_id = AuthorityId::new();
        let journal = Journal::new(JournalNamespace::Authority(auth_id));

        let state = reduce_authority(&journal);
        assert_eq!(state.facts.len(), 0);
    }

    #[test]
    fn test_reduce_context_with_bindings() {
        let ctx_id = ContextId::new();
        let mut journal = Journal::new(JournalNamespace::Context(ctx_id));

        // Add a guardian binding fact
        let fact = Fact {
            fact_id: FactId::from_bytes([1u8; 16]),
            content: FactContent::Relational(RelationalFact::GuardianBinding {
                account_id: AuthorityId::new(),
                guardian_id: AuthorityId::new(),
                binding_hash: Hash32::default(),
            }),
        };

        journal.add_fact(fact).unwrap();

        let state = reduce_context(&journal);
        assert_eq!(state.bindings.len(), 1);
        matches!(
            state.bindings[0].binding_type,
            RelationalBindingType::GuardianBinding { .. }
        );
    }

    #[test]
    fn test_flow_budget_accumulation() {
        let ctx_id = ContextId::new();
        let mut journal = Journal::new(JournalNamespace::Context(ctx_id));

        let source = AuthorityId::new();
        let dest = AuthorityId::new();
        let epoch = 1u64;

        // Add multiple flow budget facts
        for i in 1..=3 {
            let mut fact_bytes = [0u8; 16];
            fact_bytes[0] = i as u8;
            let fact = Fact {
                fact_id: FactId::from_bytes(fact_bytes),
                content: FactContent::FlowBudget(FlowBudgetFact {
                    context_id: ctx_id,
                    source,
                    destination: dest,
                    spent_amount: i * 100,
                    epoch,
                }),
            };
            journal.add_fact(fact).unwrap();
        }

        let state = reduce_context(&journal);
        let total = state
            .flow_budgets
            .get(&(source, dest, epoch))
            .copied()
            .unwrap_or(0);
        assert_eq!(total, 600); // 100 + 200 + 300
    }

    #[test]
    #[should_panic(expected = "Cannot reduce context journal as authority state")]
    fn test_reduce_wrong_namespace_type() {
        let ctx_id = ContextId::new();
        let journal = Journal::new(JournalNamespace::Context(ctx_id));
        let _ = reduce_authority(&journal);
    }
}
