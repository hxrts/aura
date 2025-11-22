//! Choreography Adapter
//!
//! Adapter for choreographic protocols with guard chain integration.

use crate::core::AgentConfig;

/// Choreography adapter for multi-party protocols
pub struct ChoreographyAdapter {
    config: AgentConfig,
}

impl ChoreographyAdapter {
    /// Create a new choreography adapter
    pub fn new(config: &AgentConfig) -> Self {
        Self {
            config: config.clone(),
        }
    }
}
