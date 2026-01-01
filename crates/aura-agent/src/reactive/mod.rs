//! # Reactive Programming Infrastructure
//!
//! Shared reactive programming primitives for Aura UI layers (TUI, web, etc.).
//! Provides FRP (Functional Reactive Programming) infrastructure independent of
//! specific UI frameworks.
//!
//! ## Purpose
//!
//! This module provides the reactive foundation that can be used by:
//! - TUI/Terminal reactive views (`aura-terminal`)
//! - Future web application UI
//! - Any other UI layer that needs reactive data flow
//!
//! ## Key Components
//!
//! - **FRP Primitives** (`frp` module):
//!   - `Dynamic<T>`: Core FRP primitive for time-varying values
//!   - FRP combinators: map, combine, filter, fold
//!   - Event subscription and propagation infrastructure
//!
//! - **Reactive Scheduler** (`scheduler` module):
//!   - `ReactiveScheduler`: Orchestrates fact ingestion and view updates
//!   - `ReactiveView` trait: Interface for views that react to journal facts
//!   - Batching logic with 5ms window for efficient updates
//!   - Glitch-freedom through topological ordering
//!
//! ## Design Principles
//!
//! - UI-agnostic: no coupling to specific UI frameworks
//! - Compositional: build complex reactive behaviors from simple primitives
//! - Async-native: built on tokio for seamless integration with Aura's async runtime
//! - Type-safe: leverages Rust's type system for correctness guarantees
//! - Deterministic: reproducible behavior for testing and simulation

pub(crate) mod app_signal_views;
pub(crate) mod frp;
pub(crate) mod pipeline;
pub(crate) mod reductions;
pub(crate) mod scheduler;
pub(crate) mod state;

// Re-export main types for convenience
pub use app_signal_views::{
    ChatSignalView, ContactsSignalView, HomeSignalView, InvitationsSignalView, RecoverySignalView,
};
pub use frp::Dynamic;
pub use pipeline::ReactivePipeline;
pub use reductions::{
    ChatReduction, GuardianDelta, GuardianReduction, HomeDelta, HomeReduction, InvitationReduction,
    RecoveryReduction,
};
pub use scheduler::{
    topological_sort_dag, AnyView, FactSource, ReactiveScheduler, ReactiveView, SchedulerConfig,
    ViewAdapter, ViewNode, ViewReduction, ViewUpdate,
};

// Re-export domain delta types from their source crates
pub use aura_chat::ChatDelta;
pub use aura_invitation::InvitationDelta;
pub use aura_recovery::RecoveryDelta;
