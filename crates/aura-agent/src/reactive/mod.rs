//! # Reactive Programming Infrastructure
//!
//! Shared reactive programming primitives for Aura UI layers (CLI, webapp, etc.).
//! Provides FRP (Functional Reactive Programming) infrastructure independent of
//! specific UI frameworks.
//!
//! ## Purpose
//!
//! This module provides the reactive foundation that can be used by:
//! - TUI/CLI reactive views (`aura-cli`)
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

pub mod fact_stream;
pub mod frp;
pub mod scheduler;

// Re-export main types for convenience
pub use fact_stream::{FactStreamAdapter, FactStreamConfig, FactStreamStats};
pub use frp::Dynamic;
pub use scheduler::{
    topological_sort_dag, AnyView, BlockDelta, BlockReduction, ChatReduction, FactSource,
    GuardianDelta, GuardianReduction, InvitationReduction, ReactiveScheduler, ReactiveView,
    RecoveryDelta, RecoveryReduction, SchedulerConfig, ViewAdapter, ViewNode, ViewReduction,
    ViewUpdate,
};

// Re-export domain delta types from their source crates
pub use aura_chat::ChatDelta;
pub use aura_invitation::InvitationDelta;
