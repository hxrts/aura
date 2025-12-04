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
//! Each query type implements the `Query` trait, which provides:
//! - `to_datalog()` - Convert to Datalog rules
//! - `parse_results()` - Parse Datalog output to typed results

mod types;

pub use types::*;

use serde::{Deserialize, Serialize};

/// A Datalog rule representation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatalogRule {
    /// Rule head (conclusion)
    pub head: String,
    /// Rule body (conditions)
    pub body: Vec<String>,
}

/// A variable binding from Datalog evaluation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Binding {
    /// Variable name
    pub name: String,
    /// Bound value (as string for FFI safety)
    pub value: String,
}

/// Trait for typed queries that compile to Datalog
pub trait Query: Sized {
    /// The result type of this query
    type Result;

    /// Convert this query to Datalog rules
    fn to_datalog(&self) -> Vec<DatalogRule>;

    /// Parse Datalog results to the typed result
    fn parse_results(bindings: Vec<Vec<Binding>>) -> Self::Result;
}
