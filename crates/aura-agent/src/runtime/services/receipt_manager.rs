//! Receipt Manager Service
//!
//! Manages receipt chains and audit trails.

use crate::core::AgentConfig;

/// Receipt manager service
pub struct ReceiptManager {
    #[allow(dead_code)] // Will be used for receipt configuration
    config: AgentConfig,
}

impl ReceiptManager {
    /// Create a new receipt manager
    pub fn new(config: &AgentConfig) -> Self {
        Self {
            config: config.clone(),
        }
    }
}
