//! Verification module for formal property checking
//!
//! This module provides tools for verifying various properties of the Aura protocol,
//! including capability soundness, privacy contracts, and protocol correctness.

pub mod capability_soundness;
pub mod lean_oracle;

pub use capability_soundness::{
    CapabilitySoundnessVerifier, CapabilityState, SoundnessProperty, SoundnessReport,
    SoundnessVerificationResult, VerificationConfig,
};

pub use lean_oracle::{
    ComparePolicy, Fact, FlowChargeInput, FlowChargeResult, JournalMergeInput, JournalMergeResult,
    JournalReduceInput, JournalReduceResult, LeanOracle, LeanOracleError, LeanOracleResult,
    OracleVersion, Ordering, TimeStamp, TimestampCompareInput, TimestampCompareResult,
};
