//! AMP runtime configuration.

use aura_core::FlowCost;

#[derive(Debug, Clone)]
pub struct AmpRuntimeConfig {
    pub default_skip_window: u32,
    pub default_flow_cost: FlowCost,
}

impl Default for AmpRuntimeConfig {
    fn default() -> Self {
        Self {
            default_skip_window: 1024,
            default_flow_cost: FlowCost::new(1),
        }
    }
}
