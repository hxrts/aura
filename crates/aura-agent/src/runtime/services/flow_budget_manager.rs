//! Flow Budget Manager Service
//!
//! Manages flow budgets per context-peer pair.
//!
//! Tracks flow budgets per (ContextId, AuthorityId) pair for charge-before-send
//! enforcement in the guard chain.

use crate::core::AgentConfig;
use aura_core::identifiers::{AuthorityId, ContextId};
use std::collections::HashMap;
use tokio::sync::RwLock;

/// A flow budget for a context-peer pair
#[derive(Debug, Clone, Default)]
pub struct FlowBudget {
    /// Maximum allowed units per epoch
    pub limit: u32,
    /// Units spent in current epoch
    pub spent: u32,
    /// Current epoch number
    pub epoch: u64,
}

impl FlowBudget {
    /// Create a new budget with the given limit
    pub fn new(limit: u32) -> Self {
        Self {
            limit,
            spent: 0,
            epoch: 0,
        }
    }

    /// Get remaining budget
    pub fn remaining(&self) -> u32 {
        self.limit.saturating_sub(self.spent)
    }

    /// Check if a charge can be applied
    pub fn can_charge(&self, cost: u32) -> bool {
        self.remaining() >= cost
    }
}

/// Flow budget manager error
#[derive(Debug, thiserror::Error)]
pub enum BudgetError {
    #[error("Insufficient budget: need {needed}, have {available}")]
    InsufficientBudget { needed: u32, available: u32 },
    #[error("Lock error")]
    LockError,
    #[error("Budget not found for context {context_id:?} and peer {peer_id:?}")]
    BudgetNotFound {
        context_id: ContextId,
        peer_id: AuthorityId,
    },
}

/// Flow budget manager service
pub struct FlowBudgetManager {
    #[allow(dead_code)] // Will be used for flow budget configuration
    config: AgentConfig,
    /// Budget storage per (ContextId, AuthorityId) pair
    budgets: RwLock<HashMap<(ContextId, AuthorityId), FlowBudget>>,
    /// Default budget limit for new pairs
    default_limit: u32,
}

impl FlowBudgetManager {
    /// Create a new flow budget manager
    pub fn new(config: &AgentConfig) -> Self {
        Self {
            config: config.clone(),
            budgets: RwLock::new(HashMap::new()),
            default_limit: 1000, // Default limit per epoch
        }
    }

    /// Get budget for a context-peer pair
    pub async fn get_budget(
        &self,
        context: ContextId,
        peer: AuthorityId,
    ) -> Result<FlowBudget, BudgetError> {
        let budgets = self.budgets.read().await;
        Ok(budgets
            .get(&(context, peer))
            .cloned()
            .unwrap_or_else(|| FlowBudget::new(self.default_limit)))
    }

    /// Charge a cost against the budget
    pub async fn charge(
        &self,
        context: ContextId,
        peer: AuthorityId,
        cost: u32,
    ) -> Result<(), BudgetError> {
        let mut budgets = self.budgets.write().await;
        let budget = budgets
            .entry((context, peer))
            .or_insert_with(|| FlowBudget::new(self.default_limit));

        if !budget.can_charge(cost) {
            return Err(BudgetError::InsufficientBudget {
                needed: cost,
                available: budget.remaining(),
            });
        }

        budget.spent += cost;
        Ok(())
    }

    /// Replenish budget for a context-peer pair
    pub async fn replenish(&self, context: ContextId, peer: AuthorityId, amount: u32) {
        let mut budgets = self.budgets.write().await;
        if let Some(budget) = budgets.get_mut(&(context, peer)) {
            budget.spent = budget.spent.saturating_sub(amount);
        }
    }

    /// Set the limit for a context-peer pair
    pub async fn set_limit(&self, context: ContextId, peer: AuthorityId, limit: u32) {
        let mut budgets = self.budgets.write().await;
        let budget = budgets
            .entry((context, peer))
            .or_insert_with(|| FlowBudget::new(limit));
        budget.limit = limit;
    }

    /// List all budgets for a context
    pub async fn list_budgets_for_context(
        &self,
        context: ContextId,
    ) -> Result<Vec<(AuthorityId, FlowBudget)>, BudgetError> {
        let budgets = self.budgets.read().await;
        Ok(budgets
            .iter()
            .filter(|((ctx, _), _)| *ctx == context)
            .map(|((_, peer), budget)| (*peer, budget.clone()))
            .collect())
    }

    /// Reset all budgets for a new epoch
    pub async fn reset_epoch(&self, new_epoch: u64) {
        let mut budgets = self.budgets.write().await;
        for budget in budgets.values_mut() {
            budget.spent = 0;
            budget.epoch = new_epoch;
        }
    }

    /// Remove budget entry for a context-peer pair.
    ///
    /// Call this when a context or peer relationship is no longer active.
    pub async fn remove_budget(&self, context: ContextId, peer: AuthorityId) -> bool {
        let mut budgets = self.budgets.write().await;
        budgets.remove(&(context, peer)).is_some()
    }

    /// Remove all budgets for a context.
    ///
    /// Call this when a context is deleted or no longer used.
    /// Returns the number of budgets removed.
    pub async fn remove_context(&self, context: ContextId) -> usize {
        let mut budgets = self.budgets.write().await;
        let before = budgets.len();
        budgets.retain(|(ctx, _), _| *ctx != context);
        before - budgets.len()
    }

    /// Cleanup stale budgets that haven't been used for several epochs.
    ///
    /// Removes budgets that are older than `stale_epochs` epochs behind current.
    /// Returns the number of budgets removed.
    pub async fn cleanup_stale(&self, current_epoch: u64, stale_epochs: u64) -> usize {
        let min_epoch = current_epoch.saturating_sub(stale_epochs);
        let mut budgets = self.budgets.write().await;
        let before = budgets.len();
        budgets.retain(|_, budget| budget.epoch >= min_epoch);
        let removed = before - budgets.len();
        if removed > 0 {
            tracing::debug!(
                removed,
                current_epoch,
                min_epoch,
                "Cleaned up stale flow budgets"
            );
        }
        removed
    }
}
