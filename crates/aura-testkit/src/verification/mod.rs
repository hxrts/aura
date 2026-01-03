//! Verification module for formal property checking
//!
//! This module provides tools for verifying various properties of the Aura protocol,
//! including capability soundness, privacy contracts, and protocol correctness.
//!
//! ## Lean Oracle (v0.4.0)
//!
//! The Lean oracle provides differential testing between Rust implementations
//! and formally verified Lean models. Key types:
//!
//! - `LeanJournal`: Full-fidelity journal with namespace and structured facts
//! - `LeanFact`: Structured fact with OrderTime, TimeStamp, and FactContent
//! - `LeanNamespace`: Authority or Context scoping
//! - `LeanTimeStamp`: 4-variant time enum (OrderClock, Physical, Logical, Range)

pub mod capability_soundness;

// Note: assertions, capabilities, strategies are also in this directory
// but they're re-exported from lib.rs directly

#[cfg(feature = "lean")]
pub mod lean_types;

#[cfg(feature = "lean")]
pub mod lean_oracle;

#[cfg(feature = "lean")]
pub mod proptest_journal;

pub use capability_soundness::{
    CapabilitySoundnessVerifier, CapabilityState, SoundnessProperty, SoundnessReport,
    SoundnessVerificationResult, VerificationConfig,
};

// Legacy types (backward compatibility)
#[cfg(feature = "lean")]
pub use lean_oracle::{
    ComparePolicy, Fact, FlowChargeInput, FlowChargeResult, JournalMergeInput, JournalMergeResult,
    JournalReduceInput, JournalReduceResult, LeanOracle, LeanOracleError, LeanOracleResult,
    OracleVersion, Ordering, TimeStamp, TimestampCompareInput, TimestampCompareResult,
};

// Full-fidelity types (v0.4.0+)
#[cfg(feature = "lean")]
pub use lean_types::{
    AttestedOp, ByteArray32, ChannelCheckpoint, ChannelId, ChannelPolicy,
    CommittedChannelEpochBump, ContextId, ConvergenceCert, DkgTranscriptCommit, Hash32,
    LeafRole, LeanFact, LeanFactContent, LeanJournal, LeanJournalMergeResult,
    LeanJournalReduceResult, LeanNamespace, LeanTimeStamp, LeakageFact, OrderTime,
    ProposedChannelEpochBump, ProtocolRelationalFact, RelationalFact, ReversionFact,
    RotateFact, SnapshotFact, TreeOpKind, AuthorityId,
};
