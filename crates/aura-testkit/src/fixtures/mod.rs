//! Layer 8: Reusable Test Fixtures - Semantic Patterns & Cleanup
//!
//! Semantic test fixtures combining builders into complete test scenarios:
//! **BiscuitFixture** (authorization tokens), **ProtocolFixture** (operation patterns),
//! **CommonFixture** (reusable patterns), **CleanupFixture** (resource management).
//!
//! **Design** (per docs/106_effect_system_and_runtime.md):
//! Fixtures provide domain-specific patterns (e.g., "create authorized peer" without
//! manual token generation). Enable fluent test writing with minimal boilerplate.

pub mod biscuit;
pub mod cleanup;
pub mod common;
pub mod protocol;
pub mod social;

pub use biscuit::*;
pub use cleanup::*;
pub use common::*;
pub use protocol::*;
pub use social::*;
