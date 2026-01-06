//! Journal effect interface for CRDT operations
//!
//! # Effect Classification
//!
//! - **Category**: Application Effect
//! - **Implementation**: `aura-journal` (Layer 2)
//! - **Usage**: Aura-specific fact-based journal operations and CRDT merging
//!
//! This is an application effect implemented in domain crates by composing
//! infrastructure effects with domain-specific business logic.

use crate::{
    types::identifiers::{AuthorityId, ContextId},
    AuraError, FlowBudget, FlowCost, Journal,
};
use async_trait::async_trait;

/// Pure trait for journal/CRDT operations
#[async_trait]
pub trait JournalEffects: Send + Sync {
    /// Merge facts using join semilattice operation
    ///
    /// Takes ownership of both journals to avoid cloning facts during merge.
    async fn merge_facts(&self, target: Journal, delta: Journal) -> Result<Journal, AuraError>;

    /// Refine capabilities using meet semilattice operation
    async fn refine_caps(
        &self,
        target: Journal,
        refinement: Journal,
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
        cost: FlowCost,
    ) -> Result<FlowBudget, AuraError>;
}

/// Blanket implementation for Arc<T> where T: JournalEffects
#[async_trait]
impl<T: JournalEffects + ?Sized> JournalEffects for std::sync::Arc<T> {
    async fn merge_facts(&self, target: Journal, delta: Journal) -> Result<Journal, AuraError> {
        (**self).merge_facts(target, delta).await
    }

    async fn refine_caps(
        &self,
        target: Journal,
        refinement: Journal,
    ) -> Result<Journal, AuraError> {
        (**self).refine_caps(target, refinement).await
    }

    async fn get_journal(&self) -> Result<Journal, AuraError> {
        (**self).get_journal().await
    }

    async fn persist_journal(&self, journal: &Journal) -> Result<(), AuraError> {
        (**self).persist_journal(journal).await
    }

    async fn get_flow_budget(
        &self,
        context: &ContextId,
        peer: &AuthorityId,
    ) -> Result<FlowBudget, AuraError> {
        (**self).get_flow_budget(context, peer).await
    }

    async fn update_flow_budget(
        &self,
        context: &ContextId,
        peer: &AuthorityId,
        budget: &FlowBudget,
    ) -> Result<FlowBudget, AuraError> {
        (**self).update_flow_budget(context, peer, budget).await
    }

    async fn charge_flow_budget(
        &self,
        context: &ContextId,
        peer: &AuthorityId,
        cost: FlowCost,
    ) -> Result<FlowBudget, AuraError> {
        (**self).charge_flow_budget(context, peer, cost).await
    }
}
