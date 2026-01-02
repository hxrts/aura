//! AMP runtime configuration.

use aura_core::FlowCost;
use std::num::NonZeroU32;

#[derive(Debug, Clone)]
pub struct AmpRuntimeConfig {
    pub default_skip_window: NonZeroU32,
    pub default_flow_cost: FlowCost,
}

impl Default for AmpRuntimeConfig {
    fn default() -> Self {
        Self {
            default_skip_window: NonZeroU32::new(1024)
                .expect("default skip window should be non-zero"),
            default_flow_cost: FlowCost::new(1),
        }
    }
}
