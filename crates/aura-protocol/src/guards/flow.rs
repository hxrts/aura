//! Flow budget guard used by higher-level protocols.
//!
//! This is a minimal implementation that threads context information
//! through the effect system so every network send can charge or
//! at least record flow usage. Future implementations can hook this
//! into the journal-backed FlowBudget CRDT described in
//! `docs/103_information_flow_budget.md`.

use aura_core::{
    effects::{FlowBudgetEffects, FlowHint},
    identifiers::{AuthorityId, ContextId},
    AuraResult, Receipt,
};

/// Guard that must run before every transport send.
#[derive(Debug)]
pub struct FlowGuard {
    hint: FlowHint,
}

impl FlowGuard {
    pub fn new(context: ContextId, peer: AuthorityId, cost: u32) -> Self {
        Self {
            hint: FlowHint::new(context, peer, cost),
        }
    }

    pub fn from_hint(hint: FlowHint) -> Self {
        Self { hint }
    }

    pub fn hint(&self) -> &FlowHint {
        &self.hint
    }

    pub async fn authorize(&self, effects: &impl FlowBudgetEffects) -> AuraResult<Receipt> {
        effects
            .charge_flow(&self.hint.context, &self.hint.peer, self.hint.cost)
            .await
    }
}
