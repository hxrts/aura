//! Flow Budget Manager Service
//!
//! Manages flow budgets per context-peer pair.

use crate::core::AgentConfig;

/// Flow budget manager service
pub struct FlowBudgetManager {
    #[allow(dead_code)] // Will be used for flow budget configuration
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
