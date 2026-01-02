//! Anti-entropy runtime configuration.

use aura_core::FlowCost;

#[derive(Debug, Clone)]
pub struct AntiEntropyRuntimeConfig {
    pub digest_cost: FlowCost,
    pub request_cost_per_cid: FlowCost,
    pub announce_cost: FlowCost,
    pub push_cost: FlowCost,
}

impl Default for AntiEntropyRuntimeConfig {
    fn default() -> Self {
        Self {
            digest_cost: FlowCost::from(10),
            request_cost_per_cid: FlowCost::from(5),
            announce_cost: FlowCost::from(5),
            push_cost: FlowCost::from(50),
        }
    }
}
