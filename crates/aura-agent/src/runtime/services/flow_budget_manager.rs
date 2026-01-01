//! Flow Budget Manager Service
//!
//! Manages flow budgets per context-peer pair.
//!
//! Tracks flow budgets per (ContextId, AuthorityId) pair for charge-before-send
//! enforcement in the guard chain.

use crate::core::AgentConfig;
use super::state::with_state_mut_validated;
use aura_core::identifiers::{AuthorityId, ContextId};
use std::collections::HashMap;
use std::sync::Arc;
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
    state: Arc<RwLock<FlowBudgetState>>,
    /// Default budget limit for new pairs
    default_limit: u32,
}

#[derive(Debug, Default)]
struct FlowBudgetState {
    budgets: HashMap<(ContextId, AuthorityId), FlowBudget>,
}

impl FlowBudgetState {
    fn validate(&self) -> Result<(), String> {
        for ((context_id, peer_id), budget) in &self.budgets {
            if budget.spent > budget.limit {
                return Err(format!(
                    "budget overspent for ({:?}, {:?}): spent {} > limit {}",
                    context_id, peer_id, budget.spent, budget.limit
                ));
            }
        }
        Ok(())
    }
}

impl FlowBudgetManager {
    /// Create a new flow budget manager
    pub fn new(config: &AgentConfig) -> Self {
        Self {
            config: config.clone(),
            state: Arc::new(RwLock::new(FlowBudgetState::default())),
            default_limit: 1000, // Default limit per epoch
        }
    }

    /// Get budget for a context-peer pair
    pub async fn get_budget(
        &self,
        context: ContextId,
        peer: AuthorityId,
    ) -> Result<FlowBudget, BudgetError> {
        let state = self.state.read().await;
        Ok(state
            .budgets
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
        let mut result = Ok(());
        with_state_mut_validated(
            &self.state,
            |state| {
                let budget = state
                    .budgets
                    .entry((context, peer))
                    .or_insert_with(|| FlowBudget::new(self.default_limit));

                if !budget.can_charge(cost) {
                    result = Err(BudgetError::InsufficientBudget {
                        needed: cost,
                        available: budget.remaining(),
                    });
                    return;
                }

                budget.spent += cost;
            },
            |state| state.validate(),
        )
        .await;
        result
    }

    /// Replenish budget for a context-peer pair
    pub async fn replenish(&self, context: ContextId, peer: AuthorityId, amount: u32) {
        with_state_mut_validated(
            &self.state,
            |state| {
                if let Some(budget) = state.budgets.get_mut(&(context, peer)) {
                    budget.spent = budget.spent.saturating_sub(amount);
                }
            },
            |state| state.validate(),
        )
        .await;
    }

    /// Set the limit for a context-peer pair
    pub async fn set_limit(&self, context: ContextId, peer: AuthorityId, limit: u32) {
        with_state_mut_validated(
            &self.state,
            |state| {
                let budget = state
                    .budgets
                    .entry((context, peer))
                    .or_insert_with(|| FlowBudget::new(limit));
                budget.limit = limit;
            },
            |state| state.validate(),
        )
        .await;
    }

    /// List all budgets for a context
    pub async fn list_budgets_for_context(
        &self,
        context: ContextId,
    ) -> Result<Vec<(AuthorityId, FlowBudget)>, BudgetError> {
        let state = self.state.read().await;
        Ok(state
            .budgets
            .iter()
            .filter(|((ctx, _), _)| *ctx == context)
            .map(|((_, peer), budget)| (*peer, budget.clone()))
            .collect())
    }

    /// Reset all budgets for a new epoch
    pub async fn reset_epoch(&self, new_epoch: u64) {
        with_state_mut_validated(
            &self.state,
            |state| {
                for budget in state.budgets.values_mut() {
                    budget.spent = 0;
                    budget.epoch = new_epoch;
                }
            },
            |state| state.validate(),
        )
        .await;
    }

    /// Remove budget entry for a context-peer pair.
    ///
    /// Call this when a context or peer relationship is no longer active.
    pub async fn remove_budget(&self, context: ContextId, peer: AuthorityId) -> bool {
        let mut removed = false;
        with_state_mut_validated(
            &self.state,
            |state| {
                removed = state.budgets.remove(&(context, peer)).is_some();
            },
            |state| state.validate(),
        )
        .await;
        removed
    }

    /// Remove all budgets for a context.
    ///
    /// Call this when a context is deleted or no longer used.
    /// Returns the number of budgets removed.
    pub async fn remove_context(&self, context: ContextId) -> usize {
        let mut removed = 0;
        with_state_mut_validated(
            &self.state,
            |state| {
                let before = state.budgets.len();
                state.budgets.retain(|(ctx, _), _| *ctx != context);
                removed = before - state.budgets.len();
            },
            |state| state.validate(),
        )
        .await;
        removed
    }

    /// Cleanup stale budgets that haven't been used for several epochs.
    ///
    /// Removes budgets that are older than `stale_epochs` epochs behind current.
    /// Returns the number of budgets removed.
    pub async fn cleanup_stale(&self, current_epoch: u64, stale_epochs: u64) -> usize {
        let min_epoch = current_epoch.saturating_sub(stale_epochs);
        let mut removed = 0;
        with_state_mut_validated(
            &self.state,
            |state| {
                let before = state.budgets.len();
                state.budgets.retain(|_, budget| budget.epoch >= min_epoch);
                removed = before - state.budgets.len();
            },
            |state| state.validate(),
        )
        .await;
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

// =============================================================================
// RuntimeService Implementation
// =============================================================================

use super::traits::{RuntimeService, ServiceError, ServiceHealth};
use super::RuntimeTaskRegistry;
use async_trait::async_trait;

#[async_trait]
impl RuntimeService for FlowBudgetManager {
    fn name(&self) -> &'static str {
        "flow_budget_manager"
    }

    async fn start(&self, _tasks: Arc<RuntimeTaskRegistry>) -> Result<(), ServiceError> {
        // FlowBudgetManager is stateless and always ready
        Ok(())
    }

    async fn stop(&self) -> Result<(), ServiceError> {
        // Clear all budgets on shutdown
        with_state_mut_validated(
            &self.state,
            |state| {
                state.budgets.clear();
            },
            |state| state.validate(),
        )
        .await;
        Ok(())
    }

    fn health(&self) -> ServiceHealth {
        ServiceHealth::Healthy
    }
}
