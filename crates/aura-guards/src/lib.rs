#![allow(
    missing_docs,
    unused_variables,
    clippy::unwrap_used,
    clippy::expect_used,
    dead_code,
    clippy::match_like_matches_macro,
    clippy::type_complexity,
    clippy::while_let_loop,
    clippy::redundant_closure,
    clippy::large_enum_variant,
    clippy::unused_unit,
    clippy::get_first,
    clippy::single_range_in_vec_init,
    clippy::disallowed_methods, // Guard chain coordinates time/random effects
    deprecated // Deprecated time/random functions used intentionally for effect coordination
)]
#![deny(clippy::await_holding_lock)]
#![deny(clippy::disallowed_types)]
//! # Aura Guards - Layer 4: Guard Chain Enforcement
//!
//! Guard chain orchestration: authorization, flow budgets, journal coupling, and leakage tracking.
//! Provides the guard chain used by Layer 4+ orchestration and transport send paths.

pub mod authorization; // Biscuit-based authorization bridge
pub mod guards;

pub use authorization::{AuthorizationResult, BiscuitAuthorizationBridge};
pub use guards::*;
