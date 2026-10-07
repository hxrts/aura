//! Per-peer flow allowance override facts (docs/111 §3.1).
//!
//! A receiver adjusts the window it grants a peer in a context by committing
//! an override fact. Overrides replicate with the owner's journal, so every
//! device of the owner authority enforces the same allowance. The newest
//! override per (context, owner, peer) wins: highest `revision`, then the
//! larger window as a deterministic tiebreak.

use crate::reducer_support::{hashed_generic_binding, reduce_typed_envelope};
use aura_core::types::identifiers::{AuthorityId, ContextId};
use aura_journal::{reduction::RelationalBinding, FactReducer};
use aura_macros::DomainFact;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Type identifier for flow allowance override facts.
pub const FLOW_ALLOWANCE_FACT_TYPE_ID: &str = "flow_allowance";

/// Override of the receive window `owner_id` grants `peer_id` in `context_id`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, DomainFact)]
#[domain_fact(type_id = "flow_allowance", schema_version = 1, context = "context_id")]
pub struct FlowAllowanceFact {
    /// Context whose receipts the window governs.
    pub context_id: ContextId,
    /// Receiver authority that grants the window.
    pub owner_id: AuthorityId,
    /// Sender authority the window applies to.
    pub peer_id: AuthorityId,
    /// Granted window size (generations per epoch).
    pub window: u64,
    /// Supersession counter: one more than the newest override the writer saw.
    pub revision: u64,
}

impl FlowAllowanceFact {
    /// Whether this override is valid in `context_id`.
    pub fn validate_for_reduction(&self, context_id: ContextId) -> bool {
        self.context_id == context_id && self.window > 0
    }

    fn supersedes(&self, other: &Self) -> bool {
        (self.revision, self.window) > (other.revision, other.window)
    }
}

/// Reduce overrides to the effective one per (context, owner, peer).
pub fn resolve_flow_allowances<'a>(
    facts: impl IntoIterator<Item = &'a FlowAllowanceFact>,
) -> BTreeMap<(ContextId, AuthorityId, AuthorityId), FlowAllowanceFact> {
    let mut effective: BTreeMap<(ContextId, AuthorityId, AuthorityId), FlowAllowanceFact> =
        BTreeMap::new();
    for fact in facts {
        if fact.window == 0 {
            continue;
        }
        let key = (fact.context_id, fact.owner_id, fact.peer_id);
        match effective.get(&key) {
            Some(current) if !fact.supersedes(current) => {}
            _ => {
                effective.insert(key, fact.clone());
            }
        }
    }
    effective
}

/// Reducer for flow allowance override facts.
pub struct FlowAllowanceFactReducer;

impl FactReducer for FlowAllowanceFactReducer {
    fn handles_type(&self) -> &'static str {
        FLOW_ALLOWANCE_FACT_TYPE_ID
    }

    fn reduce_envelope(
        &self,
        context_id: ContextId,
        envelope: &aura_core::types::facts::FactEnvelope,
    ) -> Option<RelationalBinding> {
        reduce_typed_envelope::<FlowAllowanceFact>(
            context_id,
            envelope,
            FLOW_ALLOWANCE_FACT_TYPE_ID,
            |fact| fact.validate_for_reduction(context_id),
            |_| hashed_generic_binding("flow-allowance", context_id, &envelope.payload),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_journal::DomainFact;

    fn fact(revision: u64, window: u64) -> FlowAllowanceFact {
        FlowAllowanceFact {
            context_id: ContextId::new_from_entropy([1; 32]),
            owner_id: AuthorityId::new_from_entropy([2; 32]),
            peer_id: AuthorityId::new_from_entropy([3; 32]),
            window,
            revision,
        }
    }

    #[test]
    fn newest_revision_wins_independent_of_order() {
        let facts = [fact(1, 64), fact(3, 8), fact(2, 512)];
        let forward = resolve_flow_allowances(facts.iter());
        let backward = resolve_flow_allowances(facts.iter().rev());
        assert_eq!(forward, backward);
        assert_eq!(forward.values().next().unwrap().window, 8);
    }

    #[test]
    fn concurrent_revisions_tiebreak_deterministically_and_zero_is_ignored() {
        let resolved = resolve_flow_allowances([fact(1, 64), fact(1, 32), fact(5, 0)].iter());
        assert_eq!(resolved.values().next().unwrap().window, 64);
    }

    #[test]
    fn envelope_roundtrip_and_reducer_validate_context() {
        let original = fact(1, 100);
        let envelope = original.to_envelope();
        assert_eq!(
            FlowAllowanceFact::from_envelope(&envelope),
            Some(original.clone())
        );
        let reducer = FlowAllowanceFactReducer;
        assert!(reducer
            .reduce_envelope(original.context_id, &envelope)
            .is_some());
        assert!(reducer
            .reduce_envelope(ContextId::new_from_entropy([9; 32]), &envelope)
            .is_none());
    }
}
