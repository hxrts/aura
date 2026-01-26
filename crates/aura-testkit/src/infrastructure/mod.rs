//! Layer 8: Test Execution Environment - Effect System, Harness, Time Control
//!
//! Foundation for test execution: effect system setup, context creation, harness utilities,
//! and controllable time management for deterministic testing (ExecutionMode::Testing/Simulation).
//!
//! **Key Components** (per docs/105_effect_system_and_runtime.md):
//! - **EffectSetup**: Compose test effect handlers for specific test scenarios
//! - **ContextCreation**: Set up authority/flow/receipt contexts for test operations
//! - **Harness**: Test execution framework with setup/teardown coordination
//! - **TimeControl**: Deterministic time stepping (no wall-clock waits, instant synchronization)
//!
//! **Testing Principles**:
//! - No flaky timing-dependent tests (time is controllable)
//! - Reproducible scenarios (seed-driven randomness)
//! - Complete effect composition (enables testing without external I/O)

pub mod context;
pub mod effects;
pub mod harness;
pub mod time;

pub use context::*;
pub use effects::*;
pub use harness::*;
pub use time::*;
