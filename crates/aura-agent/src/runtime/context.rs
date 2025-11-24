//! Execution context for effect operations
//!
//! Provides execution context management for authority-scoped operations,
//! tracking flow budgets, leakage controls, and execution metadata.

use aura_core::effects::ExecutionMode;
use aura_core::identifiers::{AuthorityId, ContextId, SessionId};
use std::collections::HashMap;

/// Context for effect execution within an authority scope
#[derive(Debug, Clone)]
pub struct EffectContext {
    authority_id: AuthorityId,
    context_id: ContextId,
    session_id: SessionId,
    execution_mode: ExecutionMode,
    flow_budget: FlowBudgetContext,
    leakage_budget: LeakageBudgetContext,
    metadata: HashMap<String, String>,
}

impl EffectContext {
    /// Create a new effect context
    pub fn new(
        authority_id: AuthorityId,
        context_id: ContextId,
        execution_mode: ExecutionMode,
    ) -> Self {
        Self {
            authority_id,
            context_id,
            session_id: SessionId::new(),
            execution_mode,
            flow_budget: FlowBudgetContext::new(),
            leakage_budget: LeakageBudgetContext::new(),
            metadata: HashMap::new(),
        }
    }

    /// Get the authority ID
    pub fn authority_id(&self) -> AuthorityId {
        self.authority_id
    }

    /// Get the context ID
    pub fn context_id(&self) -> ContextId {
        self.context_id
    }

    /// Get the session ID
    pub fn session_id(&self) -> SessionId {
        self.session_id
    }

    /// Get the execution mode
    pub fn execution_mode(&self) -> ExecutionMode {
        self.execution_mode
    }

    /// Get the flow budget context
    pub fn flow_budget(&self) -> &FlowBudgetContext {
        &self.flow_budget
    }

    /// Get mutable flow budget context
    pub fn flow_budget_mut(&mut self) -> &mut FlowBudgetContext {
        &mut self.flow_budget
    }

    /// Get the leakage budget context
    pub fn leakage_budget(&self) -> &LeakageBudgetContext {
        &self.leakage_budget
    }

    /// Get mutable leakage budget context
    pub fn leakage_budget_mut(&mut self) -> &mut LeakageBudgetContext {
        &mut self.leakage_budget
    }

    /// Set metadata
    pub fn set_metadata(&mut self, key: String, value: String) {
        self.metadata.insert(key, value);
    }

    /// Get metadata
    pub fn get_metadata(&self, key: &str) -> Option<&String> {
        self.metadata.get(key)
    }

    /// Get all metadata
    pub fn metadata(&self) -> &HashMap<String, String> {
        &self.metadata
    }

    /// Create a child context with a new context ID
    pub fn create_child(&self, context_id: ContextId) -> Self {
        Self {
            authority_id: self.authority_id,
            context_id,
            session_id: SessionId::new(), // New session for child
            execution_mode: self.execution_mode,
            flow_budget: self.flow_budget.clone(),
            leakage_budget: self.leakage_budget.clone(),
            metadata: self.metadata.clone(),
        }
    }

    /// Check if this context is valid for the given session
    pub fn is_valid_for_session(&self, _session_id: SessionId) -> bool {
        // Stub validation - will be expanded with actual session validation
        true
    }
}

/// Flow budget tracking context
#[derive(Debug, Clone)]
pub struct FlowBudgetContext {
    total_limit: u64,
    spent: u64,
    per_operation_limits: HashMap<String, u64>,
}

impl FlowBudgetContext {
    /// Create a new flow budget context
    pub fn new() -> Self {
        Self {
            total_limit: 10000, // Default limit
            spent: 0,
            per_operation_limits: HashMap::new(),
        }
    }

    /// Create with custom total limit
    pub fn with_limit(limit: u64) -> Self {
        Self {
            total_limit: limit,
            spent: 0,
            per_operation_limits: HashMap::new(),
        }
    }

    /// Get the total limit
    pub fn total_limit(&self) -> u64 {
        self.total_limit
    }

    /// Get the amount spent
    pub fn spent(&self) -> u64 {
        self.spent
    }

    /// Get remaining budget
    pub fn remaining(&self) -> u64 {
        self.total_limit.saturating_sub(self.spent)
    }

    /// Check if there's enough budget for an operation
    pub fn can_spend(&self, amount: u64) -> bool {
        self.remaining() >= amount
    }

    /// Spend from the budget
    pub fn spend(&mut self, amount: u64) -> Result<(), FlowBudgetError> {
        if !self.can_spend(amount) {
            return Err(FlowBudgetError::InsufficientBudget {
                requested: amount,
                available: self.remaining(),
            });
        }

        self.spent += amount;
        Ok(())
    }

    /// Set operation-specific limit
    pub fn set_operation_limit(&mut self, operation: String, limit: u64) {
        self.per_operation_limits.insert(operation, limit);
    }

    /// Get operation-specific limit
    pub fn operation_limit(&self, operation: &str) -> Option<u64> {
        self.per_operation_limits.get(operation).copied()
    }
}

impl Default for FlowBudgetContext {
    fn default() -> Self {
        Self::new()
    }
}

/// Leakage budget tracking context
#[derive(Debug, Clone)]
pub struct LeakageBudgetContext {
    metadata_budget: u64,
    timing_budget: u64,
    metadata_spent: u64,
    timing_spent: u64,
}

impl LeakageBudgetContext {
    /// Create a new leakage budget context
    pub fn new() -> Self {
        Self {
            metadata_budget: 1000,
            timing_budget: 1000,
            metadata_spent: 0,
            timing_spent: 0,
        }
    }

    /// Get metadata budget remaining
    pub fn metadata_remaining(&self) -> u64 {
        self.metadata_budget.saturating_sub(self.metadata_spent)
    }

    /// Get timing budget remaining
    pub fn timing_remaining(&self) -> u64 {
        self.timing_budget.saturating_sub(self.timing_spent)
    }

    /// Spend metadata budget
    pub fn spend_metadata(&mut self, amount: u64) -> Result<(), LeakageBudgetError> {
        if self.metadata_remaining() < amount {
            return Err(LeakageBudgetError::InsufficientMetadataBudget {
                requested: amount,
                available: self.metadata_remaining(),
            });
        }

        self.metadata_spent += amount;
        Ok(())
    }

    /// Spend timing budget
    pub fn spend_timing(&mut self, amount: u64) -> Result<(), LeakageBudgetError> {
        if self.timing_remaining() < amount {
            return Err(LeakageBudgetError::InsufficientTimingBudget {
                requested: amount,
                available: self.timing_remaining(),
            });
        }

        self.timing_spent += amount;
        Ok(())
    }
}

impl Default for LeakageBudgetContext {
    fn default() -> Self {
        Self::new()
    }
}

/// Flow budget errors
#[derive(Debug, thiserror::Error)]
pub enum FlowBudgetError {
    #[error("Insufficient budget: requested {requested}, available {available}")]
    InsufficientBudget { requested: u64, available: u64 },
}

/// Leakage budget errors
#[derive(Debug, thiserror::Error)]
pub enum LeakageBudgetError {
    #[error("Insufficient metadata budget: requested {requested}, available {available}")]
    InsufficientMetadataBudget { requested: u64, available: u64 },
    #[error("Insufficient timing budget: requested {requested}, available {available}")]
    InsufficientTimingBudget { requested: u64, available: u64 },
}
