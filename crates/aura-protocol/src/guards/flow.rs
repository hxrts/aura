//! Flow budget guard used by higher-level protocols.
//!
//! This is a minimal implementation that threads context information
//! through the effect system so every network send can charge or
//! at least record flow usage. Future implementations can hook this
//! into the journal-backed FlowBudget CRDT described in
//! `docs/103_information_flow_budget.md`.

use async_trait::async_trait;
use aura_core::{identifiers::ContextId, AuraResult, DeviceId, Receipt};
use serde::{Deserialize, Serialize};

/// Hint describing which flow bucket should be charged before a send.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlowHint {
    pub context: ContextId,
    pub peer: DeviceId,
    pub cost: u32,
}

impl FlowHint {
    pub fn new(context: ContextId, peer: DeviceId, cost: u32) -> Self {
        Self {
            context,
            peer,
            cost,
        }
    }
}

/// Trait implemented by effect systems that can charge flow budgets.
#[async_trait]
pub trait FlowBudgetEffects: Send + Sync {
    async fn charge_flow(
        &self,
        context: &ContextId,
        peer: &DeviceId,
        cost: u32,
    ) -> AuraResult<Receipt>;
}

/// Guard that must run before every transport send.
#[derive(Debug)]
pub struct FlowGuard {
    hint: FlowHint,
}

impl FlowGuard {
    pub fn new(context: ContextId, peer: DeviceId, cost: u32) -> Self {
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
