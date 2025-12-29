//! Mock journal effect handlers for testing
//!
//! # Blocking Lock Usage
//!
//! Uses `std::sync::Mutex` because this is Layer 8 test infrastructure where:
//! 1. Tests run in controlled single-threaded contexts
//! 2. Lock contention is not a concern in test scenarios
//! 3. Simpler synchronous API is preferred for test clarity

#![allow(clippy::disallowed_types)]

use async_lock::RwLock;
use async_trait::async_trait;
use aura_core::{
    effects::JournalEffects, epochs::Epoch, AuraError, AuthorityId, ContextId, FlowBudget,
    Journal as CoreJournal,
};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// Mock journal for testing
#[derive(Debug)]
pub struct Journal {
    pub entries: Vec<String>,
}

/// Mock journal handler for testing
#[derive(Debug)]
pub struct MockJournalHandler {
    journal: Arc<RwLock<Journal>>,
    flow_budgets: Arc<RwLock<HashMap<(ContextId, AuthorityId), FlowBudget>>>,
    operation_counter: Arc<Mutex<u64>>,
}

impl MockJournalHandler {
    pub fn new() -> Self {
        Self {
            journal: Arc::new(RwLock::new(Journal {
                entries: Vec::new(),
            })),
            flow_budgets: Arc::new(RwLock::new(HashMap::new())),
            operation_counter: Arc::new(Mutex::new(0)),
        }
    }

    /// Get all journal entries for testing
    pub async fn get_entries(&self) -> Vec<String> {
        let journal = self.journal.read().await;
        journal.entries.clone()
    }

    /// Clear all journal entries for testing
    pub async fn clear_entries(&self) {
        let mut journal = self.journal.write().await;
        journal.entries.clear();
    }

    /// Get flow budget count for testing
    pub async fn flow_budget_count(&self) -> usize {
        let budgets = self.flow_budgets.read().await;
        budgets.len()
    }
}

impl Default for MockJournalHandler {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl JournalEffects for MockJournalHandler {
    async fn merge_facts(
        &self,
        target: &CoreJournal,
        delta: &CoreJournal,
    ) -> Result<CoreJournal, AuraError> {
        // Mock implementation: create a new journal with combined facts
        let mut result = target.clone();
        result.merge_facts(delta.facts.clone());
        Ok(result)
    }

    async fn refine_caps(
        &self,
        target: &CoreJournal,
        refinement: &CoreJournal,
    ) -> Result<CoreJournal, AuraError> {
        // Mock implementation: apply refinements to capabilities
        let mut result = target.clone();
        result.refine_caps(refinement.caps.clone());
        Ok(result)
    }

    async fn get_journal(&self) -> Result<CoreJournal, AuraError> {
        // Mock implementation: return a journal based on our mock state
        let _journal = self.journal.read().await;

        // For testing, just return a default journal
        // In a real implementation, we'd convert our internal state to the CoreJournal format
        Ok(CoreJournal::new())
    }

    async fn persist_journal(&self, _journal: &CoreJournal) -> Result<(), AuraError> {
        // Mock implementation: store journal entries
        let mut our_journal = self.journal.write().await;
        let mut counter = self.operation_counter.lock().unwrap();
        *counter += 1;

        // Add a mock entry representing this journal persistence
        our_journal
            .entries
            .push(format!("persist_journal_{}", counter));

        Ok(())
    }

    async fn get_flow_budget(
        &self,
        context: &ContextId,
        peer: &AuthorityId,
    ) -> Result<FlowBudget, AuraError> {
        let budgets = self.flow_budgets.read().await;
        let key = (*context, *peer);

        Ok(budgets.get(&key).cloned().unwrap_or_else(|| {
            // Default flow budget for testing
            FlowBudget::new(1000, Epoch(0))
        }))
    }

    async fn update_flow_budget(
        &self,
        context: &ContextId,
        peer: &AuthorityId,
        budget: &FlowBudget,
    ) -> Result<FlowBudget, AuraError> {
        let mut budgets = self.flow_budgets.write().await;
        let key = (*context, *peer);
        budgets.insert(key, *budget);
        Ok(*budget)
    }

    async fn charge_flow_budget(
        &self,
        context: &ContextId,
        peer: &AuthorityId,
        cost: u32,
    ) -> Result<FlowBudget, AuraError> {
        let mut budgets = self.flow_budgets.write().await;
        let key = (*context, *peer);

        let mut budget = budgets
            .get(&key)
            .cloned()
            .unwrap_or_else(|| FlowBudget::new(1000, Epoch(0)));

        budget.spent += cost as u64;
        budgets.insert(key, budget);

        Ok(budget)
    }
}
