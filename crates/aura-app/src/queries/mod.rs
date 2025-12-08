//! # Query Module
//!
//! Typed queries that compile to Datalog for execution against the journal.
//!
//! ## Architecture
//!
//! Queries are the "read" side of the CQRS pattern:
//! - Intents (write) → Journal → Facts
//! - Queries (read) → Journal → Views
//!
//! Each query type implements the `aura_core::Query` trait, which provides:
//! - `to_datalog()` - Convert to Datalog program
//! - `required_capabilities()` - Biscuit capabilities needed
//! - `dependencies()` - Fact predicates for invalidation tracking
//! - `parse()` - Parse Datalog output to typed results
//!
//! ## Query-Signal Integration
//!
//! Queries integrate with the reactive system via `ReactiveEffects::register_query()`.
//! When facts matching a query's `dependencies()` change, the query is automatically
//! re-evaluated and the bound signal is updated.
//!
//! The `BoundSignal<Q>` type pairs a signal with its source query, enabling
//! automatic invalidation tracking and re-evaluation.

mod bound_signal;
mod types;

pub use bound_signal::*;
pub use types::*;

// Re-export core query types for convenience
pub use aura_core::query::{
    DatalogBindings, DatalogFact, DatalogProgram, DatalogRow, DatalogRule, DatalogValue,
    FactPredicate, Query, QueryCapability, QueryParseError,
};
