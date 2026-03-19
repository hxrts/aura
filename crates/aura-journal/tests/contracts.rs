//! Boundary contracts: tree integrity, fact encoding stability, recovery
//! reconstruction.
//!
//! These tests verify cross-module guarantees that higher layers depend on.
//! A failure here means the journal's public API contract is broken.

#[allow(clippy::expect_used, clippy::uninlined_format_args)]
#[path = "contracts/authority_tree_integrity.rs"]
mod authority_tree_integrity;

#[allow(clippy::expect_used, missing_docs)]
#[path = "contracts/fact_encoding_stability.rs"]
mod fact_encoding_stability;

#[path = "contracts/recovery_amp_reconstruction.rs"]
mod recovery_amp_reconstruction;
