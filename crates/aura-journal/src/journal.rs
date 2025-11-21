//! Legacy journal module - redirects to new fact-based journal
//!
//! This module previously contained the graph-based KeyNode/KeyEdge journal implementation.
//! As part of the authority-centric architecture refactoring, the journal system has been
//! completely replaced with a fact-based CRDT model.
//!
//! ## Migration Guide
//!
//! The new journal system is organized as follows:
//! - `fact_journal`: Core fact-based journal implementation
//! - `fact`: Fact types (AttestedOp, RelationalFact, etc.)
//! - `reduction`: Deterministic state reduction from facts
//! - `commitment_integration`: Bridge between commitment tree and facts
//!
//! For new code, use:
//! ```ignore
//! use aura_journal::fact_journal::{Journal, JournalNamespace};
//! use aura_journal::fact::{Fact, FactContent};
//! ```

// Re-export fact-based types for compatibility during transition
pub use crate::fact_journal::*;
