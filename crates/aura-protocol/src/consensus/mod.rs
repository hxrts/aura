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

pub mod choreography;
pub mod commit_fact;
pub mod coordinator;
pub mod witness;

// Re-export core types
pub use commit_fact::{CommitFact, ConsensusId};
pub use coordinator::{ConsensusCoordinator, ConsensusInstance};
pub use witness::{WitnessMessage, WitnessSet, WitnessShare};

use aura_core::{AuthorityId, Hash32, Result};
use aura_relational::prestate::Prestate;
use serde::Serialize;

/// Run consensus on an operation with the specified witnesses
///
/// This is the main entry point for running Aura Consensus on an operation.
/// It replaces the stub implementation in aura-relational.
pub async fn run_consensus<T: Serialize>(
    prestate: &Prestate,
    operation: &T,
    witnesses: Vec<AuthorityId>,
    threshold: u16,
) -> Result<CommitFact> {
    // Create coordinator
    let mut coordinator = ConsensusCoordinator::new();

    // Start consensus instance
    let instance_id = coordinator
        .start_consensus(prestate.clone(), operation, witnesses, threshold)
        .await?;

    // Run the consensus protocol
    let commit_fact = coordinator.run_protocol(instance_id).await?;

    Ok(commit_fact)
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
