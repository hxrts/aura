//! Anti-entropy runtime configuration.

#[derive(Debug, Clone)]
pub struct AntiEntropyRuntimeConfig {
    pub digest_cost: u32,
    pub request_cost_per_cid: u32,
    pub announce_cost: u32,
    pub push_cost: u32,
}

impl Default for AntiEntropyRuntimeConfig {
    fn default() -> Self {
        Self {
            digest_cost: 10,
            request_cost_per_cid: 5,
            announce_cost: 5,
            push_cost: 50,
        }
    }
}
