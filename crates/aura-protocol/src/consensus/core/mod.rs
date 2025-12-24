//! Pure Consensus Core - Effect-Free State Machine
//!
//! This module contains the pure, effect-free consensus state machine that can be
//! tested directly against Quint ITF traces and corresponds to Lean definitions.
//!
//! ## Quint Correspondence
//! - File: verification/quint/protocol_consensus.qnt
//! - Section: TYPES, STATE, EXPOSE (semantic interface)
//!
//! ## Lean Correspondence
//! - File: verification/lean/Aura/Consensus/Types.lean
//! - File: verification/lean/Aura/Consensus/Agreement.lean
//!
//! ## Design Principles
//!
//! 1. **Pure functions only**: No async, no effects, no I/O
//! 2. **Deterministic**: Same inputs always produce same outputs
//! 3. **Verifiable**: All transitions map to Quint actions
//! 4. **Invariant-checked**: Every transition maintains well-formedness

// Production modules
pub mod state;
pub mod transitions;
pub mod validation;

// Verification infrastructure (not compiled into production)
pub mod verification;

// Re-export core types for convenience
pub use state::{
    ConsensusPhase, ConsensusState, PathSelection, ShareData, ShareProposal, WitnessParticipation,
};
pub use transitions::{
    apply_share, complete_via_fallback, fail_consensus, gossip_shares, start_consensus,
    trigger_fallback, TransitionResult,
};
pub use validation::{
    check_invariants, is_equivocator, shares_consistent, validate_commit, validate_share,
    ValidationError,
};

// Verification infrastructure organization:
// - verification/quint_mapping.rs: Quint ITF correspondence (simulation feature)
// - verification/kani_proofs.rs: Bounded model checking (Kani only)
// - tests/common/reference.rs: Reference implementations for Lean-proven primitives
// - tests/common/divergence.rs: Divergence reporting for ITF conformance tests
// - tests/common/itf_loader.rs: ITF trace loading for Quint model checking
