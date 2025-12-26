#![deny(clippy::await_holding_lock)]
#![deny(clippy::disallowed_types)]
//! # Aura Guards - Layer 4: Guard Chain Enforcement
//!
//! Guard chain orchestration: authorization, flow budgets, journal coupling, and leakage tracking.
//! Provides the guard chain used by Layer 4+ orchestration and transport send paths.

pub mod authorization; // Biscuit-based authorization bridge
pub mod guards;
pub mod prelude;

pub use authorization::{AuthorizationResult, BiscuitAuthorizationBridge};
pub use guards::*;
