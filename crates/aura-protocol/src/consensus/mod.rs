//! Layer 4: Aura Consensus Implementation - Strong Agreement
//!
//! Strong-agreement consensus protocol for distributed multi-authority coordination.
//! Sole mechanism for distributed agreement in Aura (per docs/104_consensus.md).
//!
//! ## Architecture
//!
//! The consensus module has been reorganized for clarity and reduced nesting:
//!
//! - **types**: Core consensus types (ConsensusId, CommitFact, ConsensusConfig)
//! - **messages**: Protocol messages and choreography definitions
//! - **witness**: Unified witness management and state tracking
//! - **frost**: FROST cryptography integration with pipelining optimization
//! - **protocol**: Main consensus protocol coordination and execution
//! - **amp**: AMP channel epoch bump consensus adapter
//! - **relational**: Relational consensus adapter for cross-authority operations
//!
//! ## Protocol Design (per docs/104_consensus.md)
//!
//! - **Single-shot consensus**: Agrees on one operation bound to a prestate
//! - **Authority-based witnesses**: Uses AuthorityId for agreement
//! - **Two-path protocol**: Fast path (1 RTT) with cached commitments, slow path (2 RTT) fallback
//! - **Monotonic progress**: Prestate commitment prevents rollback
//! - **FROST authentication**: Threshold signatures prove witness agreement
//!
//! ## Integration Points
//!
//! - **Journal integration**: Emits CommitFact for immutable fact journals
//! - **Guard chain**: Messages flow through CapGuard → FlowGuard → Journal
//! - **Relational contexts**: Multi-authority facts enable cross-authority accountability
//! - **Effect system**: Uses PhysicalTimeEffects and RandomEffects for deterministic testing

// Core modules
pub mod frost;
pub mod messages;
pub mod protocol;
pub mod types;
pub mod witness;

// Adapters
pub mod amp;
pub mod relational;

// Re-export core types
pub use messages::{
    ConsensusError, ConsensusMessage, ConsensusPhase, ConsensusRequest, ConsensusResponse,
};
pub use protocol::{run_consensus, ConsensusProtocol};
pub use types::{CommitFact, ConflictFact, ConsensusConfig, ConsensusId, ConsensusResult};
pub use witness::{WitnessInstance, WitnessSet, WitnessState, WitnessTracker};

// Re-export AMP adapter functions
pub use amp::{
    finalize_amp_bump_with_journal, finalize_amp_bump_with_journal_default,
    run_amp_channel_epoch_bump,
};

// Re-export relational consensus
pub use relational::{
    run_consensus as run_relational_consensus,
    run_consensus_with_config as run_relational_consensus_with_config, RelationalConsensusBuilder,
};

use aura_core::{hash, Hash32, Result};

/// Hash an operation for consensus
pub(crate) fn hash_operation(bytes: &[u8]) -> Result<Hash32> {
    let mut hasher = hash::hasher();
    hasher.update(b"AURA_CONSENSUS_OP");
    hasher.update(bytes);
    Ok(Hash32(hasher.finalize()))
}
