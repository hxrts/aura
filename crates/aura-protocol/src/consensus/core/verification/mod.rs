//! Verification Infrastructure
//!
//! Contains verification-only code that is NOT compiled into production builds:
//!
//! - **quint_mapping**: Quint ITF trace correspondence (simulation feature only)
//! - **kani_proofs**: Bounded model checking proofs (Kani only)
//!
//! These modules help verify the pure consensus state machine against
//! formal specifications but are excluded from production binaries.
//!
//! ## Why These Files Live in src/ (Not tests/)
//!
//! **quint_mapping.rs**: Implements `QuintMappable` trait for consensus types.
//! Due to Rust's orphan rule, trait implementations must be in either the crate
//! that defines the trait OR the crate that defines the type. Since `QuintMappable`
//! is defined in `aura-core` and consensus types are defined here, the impls must
//! live in this crate's `src/` directory - they cannot be in `tests/`.
//!
//! **kani_proofs.rs**: Kani's tooling expects `#[cfg(kani)]` modules in `src/`.
//! Kani compiles the crate with `--cfg kani` and instruments the code; it does
//! not run integration tests. The proofs must be in the library to access internal
//! state machine functions.
//!
//! ## Test Infrastructure in tests/common/
//!
//! Pure test utilities that don't implement traits on library types live in
//! `tests/common/`: `reference.rs`, `divergence.rs`, `itf_loader.rs`.

// Quint simulation mapping (feature-gated)
#[cfg(feature = "simulation")]
pub mod quint_mapping;

// Kani bounded model checking proofs - only compiled when running Kani
#[cfg(kani)]
pub mod kani_proofs;
