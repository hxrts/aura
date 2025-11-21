//! Journal effect interface for CRDT operations

use crate::{
    identifiers::{AuthorityId, ContextId},
    AuraError, FlowBudget, Journal,
};
use async_trait::async_trait;

/// Pure trait for journal/CRDT operations
#[async_trait]
pub trait JournalEffects: Send + Sync {
    /// Merge facts using join semilattice operation
    async fn merge_facts(&self, target: &Journal, delta: &Journal) -> Result<Journal, AuraError>;

    /// Refine capabilities using meet semilattice operation
    async fn refine_caps(
        &self,
        target: &Journal,
        refinement: &Journal,
    ) -> Result<Journal, AuraError>;

    /// Get current journal state
    async fn get_journal(&self) -> Result<Journal, AuraError>;

    /// Persist journal state
    async fn persist_journal(&self, journal: &Journal) -> Result<(), AuraError>;

    /// Get FlowBudget for a (context, peer) pair
    async fn get_flow_budget(
        &self,
        context: &ContextId,
        peer: &AuthorityId,
    ) -> Result<FlowBudget, AuraError>;

    /// Update FlowBudget for a (context, peer) pair using CRDT merge
    async fn update_flow_budget(
        &self,
        context: &ContextId,
        peer: &AuthorityId,
        budget: &FlowBudget,
    ) -> Result<FlowBudget, AuraError>;

    /// Charge flow budget and return updated budget
    /// This is an atomic operation that checks headroom and charges if possible
    async fn charge_flow_budget(
        &self,
        context: &ContextId,
        peer: &AuthorityId,
        cost: u32,
    ) -> Result<FlowBudget, AuraError>;
}
