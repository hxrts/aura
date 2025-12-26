//! Aura Consensus prelude.
//!
//! Curated re-exports for consensus orchestration.

pub use crate::consensus::protocol::{run_consensus, ConsensusParams};
pub use crate::consensus::types::{CommitFact, ConsensusError};
pub use crate::consensus::{ConsensusId, ConsensusProtocol};

/// Composite effect requirements for consensus orchestration.
pub trait ConsensusEffects:
    aura_core::effects::RandomEffects + aura_core::effects::time::PhysicalTimeEffects
{
}

impl<T> ConsensusEffects for T where
    T: aura_core::effects::RandomEffects + aura_core::effects::time::PhysicalTimeEffects
{
}
