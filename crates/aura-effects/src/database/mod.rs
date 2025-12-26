//! Query Layer - Datalog queries using Biscuit's engine
//!
//! This module only exposes the stateless Datalog query wrapper. Indexed journal
//! handlers live in Layer 6 runtime crates.

pub mod query;

pub use query::{AuraQuery, FactTerm, QueryError, QueryResult};
