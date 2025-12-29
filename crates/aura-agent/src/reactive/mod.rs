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

pub mod app_signal_views;
pub mod frp;
pub mod pipeline;
pub mod scheduler;

// Re-export main types for convenience
pub use app_signal_views::{
    HomeSignalView, ChatSignalView, ContactsSignalView, InvitationsSignalView, RecoverySignalView,
};
pub use frp::Dynamic;
pub use pipeline::ReactivePipeline;
pub use scheduler::{
    topological_sort_dag, AnyView, HomeDelta, HomeReduction, ChatReduction, FactSource,
    GuardianDelta, GuardianReduction, InvitationReduction, ReactiveScheduler, ReactiveView,
    RecoveryReduction, SchedulerConfig, ViewAdapter, ViewNode, ViewReduction, ViewUpdate,
};

// Re-export domain delta types from their source crates
pub use aura_chat::ChatDelta;
pub use aura_invitation::InvitationDelta;
pub use aura_recovery::RecoveryDelta;
