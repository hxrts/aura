//! # TUI Reactive Module
//!
//! Reactive data bindings for the TUI. This module provides:
//! - Query types that generate Biscuit Datalog queries
//! - View dynamics that subscribe to database changes
//! - Type-safe conversion from query results to TUI data
//! - Query executor for executing queries and managing subscriptions
//!
//! ## Architecture Note
//!
//! FRP primitives (Dynamic<T>) and journal integration are in `aura-agent::reactive`.
//! The reactive pipeline is wired up in `TuiContext::build()`:
//! - `FactStreamAdapter` receives facts
//! - `ReactiveScheduler` orchestrates view updates
//! - Domain reductions (`ChatReduction`, etc.) convert facts to view deltas
//! - Views apply deltas via `apply_delta()`
//!
//! ## Integration Points
//!
//! - `aura_agent::reactive::FactStreamAdapter` - WASM-compatible fact streaming
//! - `aura_agent::reactive::ReactiveScheduler` - Fact batching and view updates
//! - `crate::tui::context::TuiContext` - Wiring and initialization

// TUI-specific reactive modules
pub mod journal_bridge; // Re-exports from aura_agent::reactive (see module docs)
pub mod signals; // Signal utilities for futures-signals integration

// Core modules
pub mod executor; // Query execution and data updates
pub mod queries;
pub mod views; // View state with delta application

// Re-exports
pub use executor::{DataUpdate, QueryExecutor};

// Re-export shared FRP primitives from aura-agent (runtime layer)
pub use aura_agent::reactive::Dynamic;
pub use queries::{
    Channel, ChannelType, ChannelsQuery, Guardian, GuardianApproval, GuardianStatus,
    GuardiansQuery, Invitation, InvitationDirection, InvitationStatus, InvitationType,
    InvitationsQuery, Message, MessagesQuery, RecoveryQuery, RecoveryState, RecoveryStatus,
    TuiQuery,
};
pub use views::{
    BlockAdjacency, BlockInfo, Contact, MySuggestion, NeighborhoodBlock, Resident, ResidentRole,
    StorageInfo, SuggestionPolicy, TraversalDepth, TraversalPosition, ViewState,
};

pub use aura_agent::reactive::ReactiveScheduler;
