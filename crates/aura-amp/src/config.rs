//! AMP runtime configuration.

#[derive(Debug, Clone)]
pub struct AmpRuntimeConfig {
    pub default_skip_window: u32,
    pub default_flow_cost: u32,
}

impl Default for AmpRuntimeConfig {
    fn default() -> Self {
        Self {
            default_skip_window: 1024,
            default_flow_cost: 1,
        }
    }
}
