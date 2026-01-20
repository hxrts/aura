//! Consensus protocol coordination and execution
//!
//! This module unifies the coordinator and choreography logic for running
//! the Aura Consensus protocol. It manages consensus instances, orchestrates
//! the message flow, and integrates with the FROST cryptography layer.
//!
//! ## Module Structure
//!
//! - `mod.rs` - Module wiring and re-exports
//! - `choreography.rs` - Choreography definition (PureScript-like DSL)
//! - `logic.rs` - ConsensusProtocol core logic
//! - `instance.rs` - Per-instance state management
//! - `coordinator.rs` - Coordinator role message processing
//! - `witness.rs` - Witness role participation
//! - `types.rs` - Protocol statistics and parameters

mod choreography;
mod coordinator;
mod guards;
mod instance;
mod logic;
mod types;
mod witness;

pub use choreography::*;
pub use guards::*;
pub use logic::ConsensusProtocol;
pub use types::{run_consensus, ConsensusParams, ProtocolStats};

// Re-export generated choreography types for execute_as pattern
pub mod runners {
    pub use super::choreography::rumpsteak_session_types_aura_consensus::aura_consensus::AuraConsensusRole;
    pub use super::choreography::rumpsteak_session_types_aura_consensus::runners::{
        execute_as, run_coordinator, run_witness, CoordinatorOutput, WitnessOutput,
    };
}
