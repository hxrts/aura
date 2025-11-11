//! Mock journal effect handler for testing

use aura_core::effects::JournalEffects;
use aura_core::{DeviceId, AccountId};
use async_trait::async_trait;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// Mock journal handler for testing
#[derive(Debug, Clone)]
pub struct MockJournalHandler {
    /// Mock journal entries
    entries: Arc<Mutex<HashMap<String, Value>>>,
    /// Mock operation counter
    operation_counter: Arc<Mutex<u64>>,
}

impl MockJournalHandler {
    /// Create a new mock journal handler
    pub fn new() -> Self {
        Self {
            entries: Arc::new(Mutex::new(HashMap::new())),
            operation_counter: Arc::new(Mutex::new(0)),
        }
    }

    /// Get all journal entries (for testing)
    pub fn get_entries(&self) -> HashMap<String, Value> {
        self.entries.lock().unwrap().clone()
    }

    /// Get entry count (for testing)
    pub fn entry_count(&self) -> usize {
        self.entries.lock().unwrap().len()
    }

    /// Clear all entries (for testing)
    pub fn clear(&self) {
        self.entries.lock().unwrap().clear();
        *self.operation_counter.lock().unwrap() = 0;
    }

    /// Get next operation ID
    fn next_operation_id(&self) -> u64 {
        let mut counter = self.operation_counter.lock().unwrap();
        *counter += 1;
        *counter
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
        target: &aura_core::Journal,
        delta: &aura_core::Journal,
    ) -> Result<aura_core::Journal, aura_core::AuraError> {
        // Mock implementation - just return target for now
        Ok(target.clone())
    }

    async fn refine_caps(
        &self,
        target: &aura_core::Journal,
        refinement: &aura_core::Journal,
    ) -> Result<aura_core::Journal, aura_core::AuraError> {
        // Mock implementation - just return target for now
        Ok(target.clone())
    }

    async fn get_journal(&self) -> Result<aura_core::Journal, aura_core::AuraError> {
        // Mock implementation - return empty journal
        Ok(aura_core::Journal::default())
    }

    async fn persist_journal(&self, journal: &aura_core::Journal) -> Result<(), aura_core::AuraError> {
        // Mock implementation - do nothing but succeed
        Ok(())
    }

    async fn get_flow_budget(
        &self,
        context: &aura_core::ContextId,
        peer: &DeviceId,
    ) -> Result<aura_core::FlowBudget, aura_core::AuraError> {
        // Mock implementation - return default flow budget
        Ok(aura_core::FlowBudget::default())
    }

    async fn update_flow_budget(
        &self,
        context: &aura_core::ContextId,
        peer: &DeviceId,
        budget: &aura_core::FlowBudget,
    ) -> Result<aura_core::FlowBudget, aura_core::AuraError> {
        // Mock implementation - just return the input budget
        Ok(budget.clone())
    }

    async fn charge_flow_budget(
        &self,
        context: &aura_core::ContextId,
        peer: &DeviceId,
        cost: u32,
    ) -> Result<aura_core::FlowBudget, aura_core::AuraError> {
        // Mock implementation - return default flow budget
        Ok(aura_core::FlowBudget::default())
    }

}