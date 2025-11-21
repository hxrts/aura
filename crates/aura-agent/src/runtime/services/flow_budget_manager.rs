//! Flow Budget Manager Service
//!
//! Manages flow budgets per context-peer pair.

use crate::core::AgentConfig;

/// Flow budget manager service
pub struct FlowBudgetManager {
    config: AgentConfig,
}

impl FlowBudgetManager {
    /// Create a new flow budget manager
    pub fn new(config: &AgentConfig) -> Self {
        Self {
            config: config.clone(),
        }
    }
}