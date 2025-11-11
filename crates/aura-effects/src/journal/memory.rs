//! Memory-based journal handler for testing
//!
//! This handler provides a simple in-memory implementation of JournalEffects
//! for testing and development.

use async_trait::async_trait;
use aura_core::effects::JournalEffects;
use aura_core::{relationships::ContextId, AuraError, DeviceId, FlowBudget, Journal};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Memory-based journal handler for testing
///
/// Stores journal state and flow budgets in memory.
#[derive(Clone)]
pub struct MemoryJournalHandler {
    /// Current journal state
    journal: Arc<RwLock<Journal>>,
    /// FlowBudget ledger keyed by (context, peer)
    flow_budgets: Arc<RwLock<HashMap<(ContextId, DeviceId), FlowBudget>>>,
}

impl MemoryJournalHandler {
    /// Create a new memory-based journal handler
    pub fn new() -> Self {
        Self {
            journal: Arc::new(RwLock::new(Journal::default())),
            flow_budgets: Arc::new(RwLock::new(HashMap::new())),
        }
    }
}

impl Default for MemoryJournalHandler {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl JournalEffects for MemoryJournalHandler {
    async fn merge_facts(&self, target: &Journal, _delta: &Journal) -> Result<Journal, AuraError> {
        // Simple stub - just return the target
        // Real implementation would use join-semilattice merge
        Ok(target.clone())
    }

    async fn refine_caps(
        &self,
        target: &Journal,
        _refinement: &Journal,
    ) -> Result<Journal, AuraError> {
        // Simple stub - just return the target
        // Real implementation would use meet-semilattice refine
        Ok(target.clone())
    }

    async fn get_journal(&self) -> Result<Journal, AuraError> {
        let journal = self.journal.read().await;
        Ok(journal.clone())
    }

    async fn persist_journal(&self, journal: &Journal) -> Result<(), AuraError> {
        let mut current = self.journal.write().await;
        *current = journal.clone();
        Ok(())
    }

    async fn get_flow_budget(
        &self,
        context: &ContextId,
        peer: &DeviceId,
    ) -> Result<FlowBudget, AuraError> {
        let budgets = self.flow_budgets.read().await;
        Ok(budgets
            .get(&(context.clone(), peer.clone()))
            .cloned()
            .unwrap_or_default())
    }

    async fn update_flow_budget(
        &self,
        context: &ContextId,
        peer: &DeviceId,
        budget: &FlowBudget,
    ) -> Result<FlowBudget, AuraError> {
        let mut budgets = self.flow_budgets.write().await;
        budgets.insert((context.clone(), peer.clone()), budget.clone());
        Ok(budget.clone())
    }

    async fn charge_flow_budget(
        &self,
        context: &ContextId,
        peer: &DeviceId,
        _cost: u32,
    ) -> Result<FlowBudget, AuraError> {
        // Simple stub - just return the current budget
        // Real implementation would check headroom and update
        let budgets = self.flow_budgets.read().await;
        Ok(budgets
            .get(&(context.clone(), peer.clone()))
            .cloned()
            .unwrap_or_default())
    }
}
