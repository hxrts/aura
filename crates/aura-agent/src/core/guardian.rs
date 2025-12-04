//! Guardian consensus policy configuration for the agent runtime.

use aura_core::epochs::Epoch;
use aura_core::AuthorityId;
use serde::{Deserialize, Serialize};

/// Guardian consensus policy loaded from agent configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GuardianConsensusPolicy {
    /// Witness authorities used for guardian consensus
    pub witnesses: Vec<AuthorityId>,
    /// Epoch to use for guardian consensus
    pub epoch: u64,
    /// Optional threshold override (defaults to majority of witnesses)
    pub threshold: Option<u16>,
}

impl GuardianConsensusPolicy {
    pub fn epoch(&self) -> Epoch {
        Epoch::from(self.epoch)
    }

    pub fn threshold(&self) -> u16 {
        if let Some(t) = self.threshold {
            return t.max(1);
        }
        let w = self.witnesses.len().max(1);
        w.div_ceil(2).min(u16::MAX as usize) as u16
    }
}
