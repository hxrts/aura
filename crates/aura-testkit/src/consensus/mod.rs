//! Consensus verification infrastructure for ITF conformance and differential testing.
//!
//! This module provides test harnesses for:
//! - ITF trace conformance (Quint model checking)
//! - Reference implementation comparison (Lean proofs)
//! - Divergence reporting and diagnostics
//!
//! # Usage
//!
//! ```ignore
//! use aura_testkit::consensus::{itf_loader, divergence, reference};
//!
//! // Load an ITF trace
//! let trace = itf_loader::load_itf_trace(Path::new("traces/consensus.itf.json"))?;
//!
//! // Compare states using reference implementations
//! let ref_result = reference::apply_share_ref(&state, proposal);
//!
//! // Report divergences
//! let diff = divergence::StateDiff::compare_instances(&expected, &actual);
//! ```

pub mod divergence;
pub mod itf_loader;
pub mod reference;

// Re-export commonly used items
pub use divergence::{DivergenceReport, FieldDiff, InstanceDiff, StateDiff};
pub use itf_loader::{load_itf_trace, parse_itf_trace, ITFMeta, ITFState, ITFTrace};
pub use reference::{
    aggregate_shares_ref, apply_share_ref, check_invariants_ref, check_threshold_ref,
    detect_equivocators_ref, fail_consensus_ref, merge_evidence_ref, shares_consistent_ref,
    state_to_evidence, trigger_fallback_ref, Evidence, ThresholdSignature, TransitionResultRef,
    Vote,
};
