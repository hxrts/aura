//! Native process-scoped locks shared by every Aura compile-fail harness.
//!
//! The host-only build-support package has no Aura dependencies, so
//! foundational tests use it without upward domain layer dependencies.
pub use aura_build_support::{ProcessLockError, TrybuildProcessLock, TRYBUILD_LOCK_FILE};
