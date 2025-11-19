//! Deterministic reduction for journals
//!
//! This module implements deterministic reduction of facts to produce
//! authority state and relational state from journal facts.

use crate::{
    fact::{AttestedOp, Fact, FactContent, FlowBudgetFact, RelationalFact},
    fact_journal::{Journal, JournalNamespace},
};
use aura_core::{
    authority::{AuthorityState, TreeState},
    identifiers::{AuthorityId, ContextId},
    Hash32,
};
use std::collections::{BTreeMap, BTreeSet};

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
                // TODO: Implement actual tree state transitions
                // For now, just track that we processed the operation
                tree_state = tree_state.apply(&[]);
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
    GuardianBinding {
        account_id: AuthorityId,
        guardian_id: AuthorityId,
    },
    RecoveryGrant {
        account_id: AuthorityId,
        guardian_id: AuthorityId,
    },
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
            // TODO: Implement proper state hashing
            Hash32::default()
        }
        JournalNamespace::Context(_) => {
            let state = reduce_context(journal);
            // TODO: Implement proper state hashing
            Hash32::default()
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
    use crate::fact::{FactId, SnapshotFact};

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
            fact_id: FactId::new(),
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
            let fact = Fact {
                fact_id: FactId::new(),
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
