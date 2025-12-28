#![deny(clippy::dbg_macro)]
#![deny(clippy::todo)]
#![allow(
    missing_docs,
    unused_variables,
    clippy::unwrap_used,
    clippy::expect_used,
    dead_code,
    clippy::match_like_matches_macro,
    clippy::type_complexity,
    clippy::while_let_loop,
    clippy::redundant_closure,
    clippy::large_enum_variant,
    clippy::unused_unit,
    clippy::get_first,
    clippy::single_range_in_vec_init,
    clippy::disallowed_methods, // Consensus coordinates time/random effects
    deprecated // Deprecated time/random functions used intentionally for effect coordination
)]
//! # Aura Consensus (Layer 4) - Strong Agreement
//!
//! Strong-agreement consensus protocol for distributed multi-authority coordination.
//! Sole mechanism for distributed agreement in Aura (per docs/104_consensus.md).
//!
//! ## Architecture
//!
//! - **types**: Core consensus types (ConsensusId, CommitFact, ConsensusConfig)
//! - **messages**: Protocol messages and choreography definitions
//! - **witness**: Unified witness management and state tracking
//! - **frost**: FROST cryptography integration with pipelining optimization
//! - **protocol**: Main consensus protocol coordination and execution
//! - **relational**: Relational consensus adapter for cross-authority operations
//!
//! Note: AMP consensus adapter is consolidated under `aura-amp`.
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

// Pure core - effect-free state machine for verification
pub mod config;
pub mod core;

// Core modules
pub mod choreography_runtime;
pub mod frost;
pub mod messages;
pub mod protocol;
pub mod types;
pub mod witness;

// Adapters
pub mod relational;

// Prelude
pub mod prelude;

// Re-export core types
pub use messages::{
    ConsensusError, ConsensusMessage, ConsensusPhase, ConsensusRequest, ConsensusResponse,
};
pub use protocol::{run_consensus, ConsensusProtocol};
pub use types::{CommitFact, ConflictFact, ConsensusConfig, ConsensusId, ConsensusResult};
// Intentionally avoid re-exporting witness internals to reduce coupling.

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
