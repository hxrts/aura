//! Common utilities for recovery operations
//!
//! Provides shared utilities for evidence building and signature aggregation.
//! Authorization is handled by the guard chain via choreography annotations.

pub mod evidence;
pub mod signatures;

pub use evidence::EvidenceBuilder;
pub use signatures::SignatureUtils;
