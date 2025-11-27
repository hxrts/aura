//! Guard effect interpreter implementations
//!
//! This module provides production implementations of the `EffectInterpreter` trait
//! that execute algebraic effect commands produced by pure guard evaluation.
//!
//! Per ADR-014's pure guard evaluation model, guards are pure functions that return
//! effect commands as data, and effect interpreters execute these commands asynchronously.
//! This enables algebraic effects, WASM compatibility, and deterministic simulation
//! while maintaining clean separation between business logic and I/O.

pub mod production_interpreter;

pub use production_interpreter::ProductionEffectInterpreter;
