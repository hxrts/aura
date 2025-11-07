//! Effects System Trait Definitions for Aura
//!
//! This module provides the foundational effect trait definitions that enable
//! deterministic testing and clean architecture across all Aura components.
//!
//! ## Design Principles
//!
//! 1. **Algebraic Effects**: Pure functions accept effects as parameters
//! 2. **Effect Isolation**: All side effects are contained within effect traits
//! 3. **Testability**: All effects can be mocked/injected for deterministic testing
//! 4. **Universal Usage**: Same effects system used by all layers
//!
//! ## Architecture
//!
//! **aura-types** (this crate): Effect trait definitions ONLY
//! **aura-protocol**: Production effect implementations
//! **aura-crypto**: Crypto-specific effect implementations
//!
//! ## Core Effect Categories
//!
//! - **TimeEffects**: Time operations for deterministic testing

pub mod time;

pub use time::{SystemTimeEffects, TimeEffects};
