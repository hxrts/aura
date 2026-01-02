//! Consensus runtime configuration.

use std::num::NonZeroU64;

#[derive(Debug, Clone)]
pub struct ConsensusRuntimeConfig {
    pub default_timeout_ms: NonZeroU64,
    pub enable_pipelining: bool,
}

impl Default for ConsensusRuntimeConfig {
    fn default() -> Self {
        Self {
            default_timeout_ms: NonZeroU64::new(30_000)
                .expect("default timeout should be non-zero"),
            enable_pipelining: true,
        }
    }
}
