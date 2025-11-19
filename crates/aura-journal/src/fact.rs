//! Fact model for the journal system
//!
//! This module defines the core fact types used in the fact-based journal.
//! Facts are immutable, ordered entries that represent state changes in the system.

pub use crate::fact_journal::{
    AttestedOp, Fact, FactContent, FactId, FactType, FlowBudgetFact, RelationalFact, SnapshotFact,
    TreeOpKind,
};

// Re-export from fact_journal to maintain a clean API
// All fact-related types are defined in fact_journal.rs to avoid circular dependencies
