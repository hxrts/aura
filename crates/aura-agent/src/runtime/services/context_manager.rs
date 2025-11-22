//! Context Manager Service
//!
//! Manages execution contexts and authority relationships.

use crate::core::AgentConfig;

/// Context manager service
pub struct ContextManager {
    config: AgentConfig,
}

impl ContextManager {
    /// Create a new context manager
    pub fn new(config: &AgentConfig) -> Self {
        Self {
            config: config.clone(),
        }
    }
}
