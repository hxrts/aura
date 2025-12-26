//! Consensus runtime configuration.

#[derive(Debug, Clone)]
pub struct ConsensusRuntimeConfig {
    pub default_timeout_ms: u64,
    pub enable_pipelining: bool,
}

impl Default for ConsensusRuntimeConfig {
    fn default() -> Self {
        Self {
            default_timeout_ms: 30_000,
            enable_pipelining: true,
        }
    }
}
