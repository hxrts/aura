//! Journal effect handlers
//!
//! This module provides standard implementations of the `JournalEffects` trait
//! defined in `aura-core`. These handlers can be used by choreographic applications
//! and other Aura components.

use aura_core::effects::JournalEffects;
use aura_core::{relationships::ContextId, AuraError, DeviceId, Epoch, FlowBudget, Journal};
use aura_macros::aura_effect_handlers;

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};

// Generate both mock and real journal handlers using the macro
aura_effect_handlers! {
    trait_name: JournalEffects,
    mock: {
        struct_name: MockJournalHandler,
        state: {
            journal: Arc<RwLock<Journal>>,
            flow_budgets: Arc<RwLock<HashMap<(ContextId, DeviceId), FlowBudget>>>,
            operation_counter: Arc<Mutex<u64>>,
        },
        features: {
            async_trait: true,
            deterministic: true,
        },
        methods: {
            merge_facts(target: &Journal, _delta: &Journal) -> Result<Journal, AuraError> => {
                // Mock implementation - just return target
                Ok(target.clone())
            },
            refine_caps(target: &Journal, _refinement: &Journal) -> Result<Journal, AuraError> => {
                // Mock implementation - just return target
                Ok(target.clone())
            },
            get_journal() -> Result<Journal, AuraError> => {
                let journal = self.journal.read().await;
                Ok(journal.clone())
            },
            persist_journal(journal: &Journal) -> Result<(), AuraError> => {
                let mut current = self.journal.write().await;
                *current = journal.clone();
                Ok(())
            },
            get_flow_budget(context: &ContextId, peer: &DeviceId) -> Result<FlowBudget, AuraError> => {
                let budgets = self.flow_budgets.read().await;
                Ok(budgets
                    .get(&(context.clone(), *peer))
                    .copied()
                    .unwrap_or_default())
            },
            update_flow_budget(context: &ContextId, peer: &DeviceId, budget: &FlowBudget) -> Result<FlowBudget, AuraError> => {
                let mut budgets = self.flow_budgets.write().await;
                budgets.insert((context.clone(), *peer), *budget);
                Ok(*budget)
            },
            charge_flow_budget(context: &ContextId, peer: &DeviceId, cost: u32) -> Result<FlowBudget, AuraError> => {
                let mut budgets = self.flow_budgets.write().await;
                let budget_key = (context.clone(), *peer);
                let mut budget = budgets.get(&budget_key).copied().unwrap_or_default();

                // Check headroom
                if budget.spent + cost as u64 > budget.limit {
                    return Err(AuraError::budget_exceeded(format!(
                        "Flow budget exceeded: spent={}, cost={}, limit={}",
                        budget.spent, cost, budget.limit
                    )));
                }

                // Charge the budget
                budget.spent += cost as u64;
                budgets.insert(budget_key, budget);

                tracing::debug!(
                    context = ?context,
                    peer = ?peer,
                    cost = cost,
                    new_spent = budget.spent,
                    limit = budget.limit,
                    "Flow budget charged in mock handler"
                );

                Ok(budget)
            },
        },
    },
    real: {
        struct_name: StandardJournalHandler,
        state: {
            operation_counter: Arc<Mutex<u64>>,
        },
        features: {
            async_trait: true,
        },
        methods: {
            merge_facts(target: &Journal, _delta: &Journal) -> Result<Journal, AuraError> => {
                // Standard implementation would use proper CRDT merge logic
                // For now, return target - this should be implemented with real domain logic
                Ok(target.clone())
            },
            refine_caps(target: &Journal, _refinement: &Journal) -> Result<Journal, AuraError> => {
                // Standard implementation would use meet semilattice logic
                // For now, return target - this should be implemented with real domain logic
                Ok(target.clone())
            },
            get_journal() -> Result<Journal, AuraError> => {
                // Standard implementation would load from persistent storage
                // For now, return default journal
                Ok(Journal::default())
            },
            persist_journal(_journal: &Journal) -> Result<(), AuraError> => {
                // Standard implementation would persist to storage backend
                // For now, succeed without doing anything
                Ok(())
            },
            get_flow_budget(_context: &ContextId, _peer: &DeviceId) -> Result<FlowBudget, AuraError> => {
                // Standard implementation would load from persistent budget store
                // For now, return default flow budget
                Ok(FlowBudget::default())
            },
            update_flow_budget(_context: &ContextId, _peer: &DeviceId, budget: &FlowBudget) -> Result<FlowBudget, AuraError> => {
                // Standard implementation would persist and return merged budget using CRDT logic
                // For now, just return the input budget
                Ok(*budget)
            },
            charge_flow_budget(context: &ContextId, peer: &DeviceId, cost: u32) -> Result<FlowBudget, AuraError> => {
                // Standard implementation with atomic headroom check and charge
                // In production, this would use persistent storage and proper CRDT merging

                // For now, simulate the operation with in-memory state
                // In production, this would:
                // 1. Load current budget from persistent store
                // 2. Check headroom atomically
                // 3. Apply the charge using CRDT merge
                // 4. Persist the updated budget

                let default_limit = 10000u32; // Default flow budget limit
                let mut budget = FlowBudget {
                    limit: default_limit as u64,
                    spent: 0,
                    epoch: Epoch(1),
                };

                // Load existing budget (simulated)
                // In production: budget = load_from_storage(context, peer)?;

                // Check headroom
                if budget.spent + cost as u64 > budget.limit {
                    return Err(AuraError::budget_exceeded(format!(
                        "Flow budget exceeded: spent={}, cost={}, limit={}",
                        budget.spent, cost, budget.limit
                    )));
                }

                // Charge the budget
                budget.spent += cost as u64;

                // Persist updated budget (simulated)
                // In production: persist_to_storage(context, peer, &budget)?;

                tracing::info!(
                    context = ?context,
                    peer = ?peer,
                    cost = cost,
                    new_spent = budget.spent,
                    limit = budget.limit,
                    "Flow budget charged in standard handler"
                );

                Ok(budget)
            },
        },
    },
}

impl MockJournalHandler {
    /// Get all journal entries (for testing)
    pub async fn get_journal_entries(&self) -> Journal {
        self.journal.read().await.clone()
    }

    /// Get entry count (for testing)
    pub async fn entry_count(&self) -> usize {
        // Simplified count - would need proper journal structure
        self.flow_budgets.read().await.len()
    }

    /// Clear all entries (for testing)
    pub async fn clear(&self) {
        {
            let mut journal = self.journal.write().await;
            *journal = Journal::default();
        }
        {
            let mut budgets = self.flow_budgets.write().await;
            budgets.clear();
        }
        {
            let mut counter = self.operation_counter.lock().await;
            *counter = 0;
        }
    }

    /// Get next operation ID
    pub async fn next_operation_id(&self) -> u64 {
        let mut counter = self.operation_counter.lock().await;
        *counter += 1;
        *counter
    }
}

impl StandardJournalHandler {
    /// Get next operation ID
    pub async fn next_operation_id(&self) -> u64 {
        let mut counter = self.operation_counter.lock().await;
        *counter += 1;
        *counter
    }
}
