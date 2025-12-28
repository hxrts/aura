//! CRDT handler utilities (Layer 2)
//!
//! Composable handler implementations that enforce CRDT semantic laws (⊔, ⊓)
//! independent of any session-type communication. These handlers provide
//! local convergence guarantees and can be embedded by higher-layer protocols.
//!
//! # Handler Selection Guide
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
//! │   └─► Use CmHandler (commutative operations with causal delivery)
//! │       Examples: Collaborative editing, operation logs, chat messages
//! │
//! └─► State-based but bandwidth-constrained?
//!     └─► Use DeltaHandler (incremental sync with fold threshold)
//!         Examples: Large journals, distributed state with many small updates
//! ```
//!
//! ## Quick Reference
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
//!
//! # The `CrdtHandler` Trait
//!
//! All handlers implement the [`CrdtHandler`] trait, providing a unified interface
//! for runtime introspection and generic handler manipulation:
//!
//! ```rust,ignore
//! use aura_journal::crdt::{CrdtHandler, CrdtSemantics, CvHandler};
//!
//! fn describe_handler<S>(handler: &impl CrdtHandler<S>) -> &'static str {
//!     match handler.semantics() {
//!         CrdtSemantics::JoinSemilattice => "State-based, monotonically increasing",
//!         CrdtSemantics::MeetSemilattice => "Constraint-based, monotonically decreasing",
//!         CrdtSemantics::OperationBased => "Causal operations with vector clocks",
//!         CrdtSemantics::DeltaBased => "Incremental updates with fold threshold",
//!     }
//! }
//! ```
//!
//! Higher-layer choreography/transport coordination lives in `aura-protocol`.

mod cm_handler;
mod cv_handler;
mod delta_handler;
mod handler_trait;
mod mv_handler;

pub use cm_handler::CmHandler;
pub use cv_handler::CvHandler;
pub use delta_handler::{DeltaHandler, JournalDeltaHandler};
pub use handler_trait::{CrdtHandler, CrdtSemantics, HandlerDiagnostics, HandlerMetrics};
pub use mv_handler::{ConstraintEvent, ConstraintResult, MultiConstraintHandler, MvHandler};
