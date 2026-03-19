//! CRDT convergence and reducer determinism tests.
//!
//! If any test here fails, replicas with the same facts will derive different
//! state — the core safety guarantee of the journal system is broken.

#[allow(clippy::expect_used, missing_docs)]
#[path = "convergence/journal_join_laws.rs"]
mod journal_join_laws;

#[allow(clippy::expect_used)]
#[path = "convergence/tree_reduction_determinism.rs"]
mod tree_reduction_determinism;

#[path = "convergence/convergence_cert.rs"]
mod convergence_cert;
