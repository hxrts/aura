//! Common CRDT Handler Trait
//!
//! Defines a unified interface for all CRDT handler types, providing
//! a consistent API while preserving the distinct mathematical semantics
//! of each handler type.
//!
//! # Handler Selection Guide
//!
//! Choosing the right handler depends on your CRDT's mathematical properties
//! and synchronization requirements:
//!
//! ## Decision Tree
//!
//! ```text
//! Is your data structure...
//! │
//! ├─► Accumulating state over time (counters, sets, logs)?
//! │   └─► Use CvHandler (join semilattice ⊔)
//! │       Examples: G-Counter, G-Set, LWW-Register, OR-Set
//! │
//! ├─► Restricting/constraining permissions or policies?
//! │   └─► Use MvHandler (meet semilattice ⊓)
//! │       Examples: Capability sets, access policies, budget limits
//! │
//! ├─► Operation-based with causal ordering requirements?
//! │   └─► Use CmHandler (commutative operations)
//! │       Examples: Collaborative editing, operation logs, chat messages
//! │
//! └─► State-based but bandwidth-constrained?
//!     └─► Use DeltaHandler (incremental sync)
//!         Examples: Large journals, distributed state with many small updates
//! ```
//!
//! ## Detailed Comparison
//!
//! | Handler | Lattice | Direction | Use When |
//! |---------|---------|-----------|----------|
//! | `CvHandler` | Join (⊔) | Monotonically increasing | Accumulating data |
//! | `MvHandler` | Meet (⊓) | Monotonically decreasing | Restricting permissions |
//! | `CmHandler` | Operations | Causal ordering | Need operation history |
//! | `DeltaHandler` | Join + Delta | Incremental | Large state, low bandwidth |
//!
//! ## Common Mistakes
//!
//! - **Using CvHandler for permissions**: Join makes sets grow, but permissions
//!   should shrink when restricted. Use MvHandler instead.
//!
//! - **Using MvHandler for counters**: Meet finds minimum, but counters should
//!   find maximum. Use CvHandler instead.
//!
//! - **Using CmHandler when order doesn't matter**: If operations naturally
//!   commute and you don't need causal ordering, CvHandler is simpler.

use std::fmt::Debug;

/// Semantic category of the CRDT handler
///
/// This enum identifies the mathematical basis of a handler,
/// which determines how state evolves and converges.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CrdtSemantics {
    /// State-based with join semilattice (monotonically increasing)
    ///
    /// State evolves via: `new_state = current.join(received)`
    /// Examples: G-Counter, G-Set, LWW-Register
    JoinSemilattice,

    /// State-based with meet semilattice (monotonically decreasing)
    ///
    /// State evolves via: `new_state = current.meet(received)`
    /// Examples: Capability sets, access policies
    MeetSemilattice,

    /// Operation-based with causal delivery guarantees
    ///
    /// Operations are buffered until causally ready, then applied.
    /// Examples: Collaborative editing, chat with ordering
    OperationBased,

    /// Delta-based for bandwidth-efficient synchronization
    ///
    /// Combines delta-based updates with periodic state folding.
    /// Examples: Large distributed journals
    DeltaBased,
}

impl CrdtSemantics {
    /// Returns true if this handler type uses join operations
    pub fn uses_join(&self) -> bool {
        matches!(
            self,
            CrdtSemantics::JoinSemilattice | CrdtSemantics::DeltaBased
        )
    }

    /// Returns true if this handler type uses meet operations
    pub fn uses_meet(&self) -> bool {
        matches!(self, CrdtSemantics::MeetSemilattice)
    }

    /// Returns true if this handler type requires causal ordering
    pub fn requires_causal_ordering(&self) -> bool {
        matches!(self, CrdtSemantics::OperationBased)
    }

    /// Human-readable description of when to use this handler type
    pub fn usage_guidance(&self) -> &'static str {
        match self {
            CrdtSemantics::JoinSemilattice => {
                "Use for accumulating data: counters, sets, logs. State grows monotonically."
            }
            CrdtSemantics::MeetSemilattice => {
                "Use for restricting data: capabilities, policies. State shrinks monotonically."
            }
            CrdtSemantics::OperationBased => {
                "Use when operation order matters: collaborative editing, chat with threading."
            }
            CrdtSemantics::DeltaBased => {
                "Use for large state with bandwidth constraints: journals, large sets."
            }
        }
    }
}

/// Common interface for all CRDT handlers
///
/// This trait provides a unified API for inspecting and interacting with
/// CRDT handlers, regardless of their specific mathematical semantics.
///
/// # Type Parameters
///
/// - `S`: The state type managed by this handler
///
/// # Design Rationale
///
/// While all handlers share this interface, their `on_recv` methods differ
/// because they accept different message types with different semantics.
/// This trait captures the common read-only and diagnostic operations.
pub trait CrdtHandler<S> {
    /// Get the semantic category of this handler
    ///
    /// This is useful for runtime introspection and debugging.
    fn semantics(&self) -> CrdtSemantics;

    /// Get an immutable reference to the current state
    fn state(&self) -> &S;

    /// Get a mutable reference to the current state
    ///
    /// # Warning
    ///
    /// Direct mutation should preserve the handler's invariants.
    /// Prefer using the handler's specific update methods when possible.
    fn state_mut(&mut self) -> &mut S;

    /// Check if the handler has pending work
    ///
    /// Returns true if there are buffered operations, uncommitted deltas,
    /// or other pending work that should be processed.
    fn has_pending_work(&self) -> bool;

    /// Get diagnostic information about the handler's state
    fn diagnostics(&self) -> HandlerDiagnostics;
}

/// Diagnostic information about a CRDT handler
#[derive(Debug, Clone)]
pub struct HandlerDiagnostics {
    /// Semantic category of the handler
    pub semantics: CrdtSemantics,
    /// Number of pending/buffered items (operations, deltas, etc.)
    pub pending_count: usize,
    /// Whether the handler is in a clean/idle state
    pub is_idle: bool,
    /// Additional handler-specific metrics
    pub metrics: HandlerMetrics,
}

/// Handler-specific metrics
#[derive(Debug, Clone, Default)]
pub struct HandlerMetrics {
    /// For CmHandler: number of applied operations
    pub applied_operations: Option<u32>,
    /// For MvHandler: number of constraints applied
    pub constraints_applied: Option<u32>,
    /// For DeltaHandler: fold threshold
    pub fold_threshold: Option<u32>,
    /// For MvHandler: number of consistency proofs received
    pub consistency_proofs: Option<u32>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_crdt_semantics() {
        assert!(CrdtSemantics::JoinSemilattice.uses_join());
        assert!(!CrdtSemantics::JoinSemilattice.uses_meet());

        assert!(CrdtSemantics::MeetSemilattice.uses_meet());
        assert!(!CrdtSemantics::MeetSemilattice.uses_join());

        assert!(CrdtSemantics::OperationBased.requires_causal_ordering());
        assert!(!CrdtSemantics::JoinSemilattice.requires_causal_ordering());

        assert!(CrdtSemantics::DeltaBased.uses_join());
    }

    #[test]
    fn test_usage_guidance() {
        let guidance = CrdtSemantics::JoinSemilattice.usage_guidance();
        assert!(guidance.contains("accumulating"));

        let guidance = CrdtSemantics::MeetSemilattice.usage_guidance();
        assert!(guidance.contains("restricting"));
    }
}
