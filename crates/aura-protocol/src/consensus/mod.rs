//! Aura Consensus Implementation
//!
//! This module provides the real Aura Consensus protocol implementation,
//! replacing the stub in aura-relational. It integrates with FROST threshold
//! signatures and produces CommitFact entries for journals.
//!
//! ## Design Principles (from docs/402_consensus.md):
//!
//! - **Single-shot consensus**: Agrees on one operation bound to a prestate
//! - **Authority-based witnesses**: Uses AuthorityId, not device IDs
//! - **Two-path protocol**: Fast path and fallback epidemic gossip
//! - **Journal integration**: Emits CommitFact for fact journals

pub mod amp;
pub mod choreography;
pub mod commit_fact;
pub mod coordinator;
pub mod relational_consensus;
pub mod witness;

// Re-export core types
pub use amp::{finalize_amp_bump_with_journal_default, run_amp_channel_epoch_bump};
pub use choreography::run_consensus_choreography;
pub use commit_fact::{CommitFact, ConsensusId};
pub use coordinator::{ConsensusCoordinator, ConsensusInstance};
pub use relational_consensus::{
    run_consensus as run_relational_consensus,
    run_consensus_with_config as run_relational_consensus_with_config,
    ConsensusConfig as RelationalConsensusConfig,
};
pub use witness::{WitnessMessage, WitnessSet, WitnessShare};

use aura_core::{hash, AuthorityId, Hash32, Prestate, Result};
use frost_ed25519::keys::{KeyPackage, PublicKeyPackage};
use serde::Serialize;
use serde_json;

/// Run consensus on an operation with the specified witnesses
///
/// This is the main entry point for running Aura Consensus on an operation
/// using the choreography-defined protocol.
pub async fn run_consensus<T: Serialize>(
    prestate: &Prestate,
    operation: &T,
    witnesses: Vec<AuthorityId>,
    threshold: u16,
    key_packages: std::collections::HashMap<AuthorityId, KeyPackage>,
    group_public_key: PublicKeyPackage,
) -> Result<CommitFact> {
    let prestate_hash = prestate.compute_hash();
    let operation_bytes = serde_json::to_vec(operation)
        .map_err(|e| aura_core::AuraError::serialization(e.to_string()))?;
    let operation_hash = hash_operation(&operation_bytes)?;

    run_consensus_choreography(
        prestate_hash,
        operation_hash,
        operation_bytes,
        witnesses,
        threshold,
        key_packages,
        group_public_key,
    )
    .await
}

/// Consensus configuration
#[derive(Debug, Clone)]
pub struct ConsensusConfig {
    /// Minimum number of witnesses required
    pub threshold: u16,
    /// Set of eligible witnesses
    pub witness_set: Vec<AuthorityId>,
    /// Timeout for consensus operations in milliseconds
    pub timeout_ms: u64,
    /// Enable fast path optimization
    pub enable_fast_path: bool,
}

impl ConsensusConfig {
    /// Create a new consensus configuration
    pub fn new(threshold: u16, witness_set: Vec<AuthorityId>) -> Self {
        Self {
            threshold,
            witness_set,
            timeout_ms: 30000, // 30 seconds default
            enable_fast_path: true,
        }
    }

    /// Check if we have sufficient witnesses
    pub fn has_quorum(&self) -> bool {
        self.witness_set.len() >= self.threshold as usize
    }
}

fn hash_operation(bytes: &[u8]) -> Result<Hash32> {
    let mut hasher = hash::hasher();
    hasher.update(b"AURA_CONSENSUS_OP");
    hasher.update(bytes);
    Ok(Hash32(hasher.finalize()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_consensus_config() {
        let witnesses = vec![AuthorityId::new(), AuthorityId::new(), AuthorityId::new()];

        let config = ConsensusConfig::new(2, witnesses);
        assert!(config.has_quorum());
        assert_eq!(config.threshold, 2);
        assert_eq!(config.timeout_ms, 30000);
    }
}
