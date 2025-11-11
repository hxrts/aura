//! Verification module for formal property checking
//!
//! This module provides tools for verifying various properties of the Aura protocol,
//! including capability soundness, privacy contracts, and protocol correctness.

pub mod capability_soundness;

pub use capability_soundness::{
    CapabilitySoundnessVerifier, SoundnessProperty, SoundnessVerificationResult,
    CapabilityState, VerificationConfig, SoundnessReport,
};