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
    clippy::disallowed_methods, // Consensus coordinates time/random effects
    deprecated // Deprecated time/random functions used intentionally for effect coordination
)]
//! # Aura Consensus - Layer 4: Strong Agreement
//!
//! Consensus protocol implementation (fast path + fallback) for Aura.

pub mod consensus;

pub use consensus::*;
