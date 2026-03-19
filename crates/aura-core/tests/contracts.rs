//! Cross-module boundary and contract tests.
//!
//! Verify the public API contracts that higher layers depend on:
//! serialization roundtrips, identifier uniqueness, deterministic
//! key derivation, and scaling behavior.

#[path = "contracts/serialization_roundtrip.rs"]
mod serialization_roundtrip;

#[path = "contracts/content_addressing.rs"]
mod content_addressing;

#[path = "contracts/dkd_determinism.rs"]
mod dkd_determinism;

#[allow(clippy::expect_used)]
#[path = "contracts/identifier_uniqueness.rs"]
mod identifier_uniqueness;

#[allow(clippy::expect_used, clippy::disallowed_methods, missing_docs)]
#[path = "contracts/consistency_scaling.rs"]
mod consistency_scaling;
