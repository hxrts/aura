//! Context Manager Service
//!
//! Manages execution contexts and authority relationships.

use crate::core::AgentConfig;

/// Context manager service
pub struct ContextManager {
    #[allow(dead_code)] // Will be used for context configuration
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
