//! Choreographic protocol definitions following docs/405_protocol_guide.md
//!
//! This module contains the core choreographic protocols organized by functional area:
//! - `dkd`: Deterministic key derivation protocols
//! - `frost`: FROST threshold signature protocols  
//! - `consensus`: Consensus and coordination protocols
//!
//! All protocols follow the protocol guide design principles:
//! - Start from global choreographic perspective
//! - Use strongly-typed messages with clear semantics
//! - Include version information in protocol messages
//! - Model explicit failure modes in choreographies

pub mod consensus;
pub mod dkd;
pub mod frost;

// Re-export all protocol functions for convenience
pub use consensus::{
    execute_broadcast_gather, execute_consensus, execute_coordinator_monitoring,
    execute_failure_recovery, execute_propose_acknowledge, ConsensusConfig, ConsensusResult,
};
pub use dkd::{execute_dkd, DkdConfig, DkdResult};
pub use frost::{execute_frost_signing, execute_threshold_unwrap, FrostConfig, FrostResult};

/// Common result type for all protocol executions following protocol guide
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum ProtocolResult {
    Dkd(DkdResult),
    Frost(FrostResult),
    Consensus(ConsensusResult),
}

impl ProtocolResult {
    /// Check if the protocol execution was successful
    pub fn is_success(&self) -> bool {
        match self {
            ProtocolResult::Dkd(result) => result.success,
            ProtocolResult::Frost(result) => result.success,
            ProtocolResult::Consensus(result) => result.success,
        }
    }
}
