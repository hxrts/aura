//! # Journal-Reactive Bridge
//!
//! Bridges journal facts to reactive views. The core implementation lives in
//! `aura-agent::reactive` and is wired up through `TuiContext`.
//!
//! ## Current Architecture
//!
//! The journal → view flow is now handled by:
//!
//! 1. **FactStreamAdapter** (`aura-agent::reactive::FactStreamAdapter`)
//!    - WASM-compatible fact streaming infrastructure
//!    - Receives facts via `notify_facts()` method
//!    - Broadcasts to subscribers
//!
//! 2. **ReactiveScheduler** (`aura-agent::reactive::ReactiveScheduler`)
//!    - Orchestrates fact ingestion and view updates
//!    - Batches facts with 5ms window for efficient updates
//!    - Maintains topological ordering for glitch-freedom
//!
//! 3. **Domain Reductions** (`aura-agent::reactive`)
//!    - `ChatReduction` → `ChatDelta` (from `aura-chat`)
//!    - `InvitationReduction` → `InvitationDelta` (from `aura-invitation`)
//!    - `BlockReduction` → `BlockDelta`
//!    - `GuardianReduction` → `GuardianDelta`
//!    - `RecoveryReduction` → `RecoveryDelta`
//!
//! 4. **ViewAdapter** (`aura-agent::reactive::ViewAdapter`)
//!    - Wraps views with reduction logic
//!    - Registered with ReactiveScheduler
//!    - Calls `view.apply_delta()` when facts are reduced
//!
//! 5. **TuiContext** (`crate::tui::context::TuiContext`)
//!    - Wires up all components on construction
//!    - Spawns background tasks for fact forwarding
//!    - Provides `fact_stream_adapter()` for feeding facts
//!    - Provides `send_facts_to_scheduler()` for direct scheduler access
//!
//! ## Usage
//!
//! To feed facts into the reactive system:
//!
//! ```ignore
//! // Option 1: Via FactStreamAdapter (WASM-compatible)
//! let facts = vec![/* journal facts */];
//! context.fact_stream_adapter().notify_facts(facts).await;
//!
//! // Option 2: Direct to scheduler
//! use aura_agent::reactive::FactSource;
//! context.send_facts_to_scheduler(FactSource::Journal(facts)).await?;
//! ```
//!
//! ## See Also
//!
//! - `aura-agent/src/reactive/mod.rs` - Reactive infrastructure
//! - `aura-agent/src/reactive/scheduler.rs` - ReactiveScheduler implementation
//! - `aura-agent/src/reactive/fact_stream.rs` - FactStreamAdapter
//! - `crate::tui::context::TuiContext::build()` - Wiring logic

// Re-export types that may be used by other modules for backwards compatibility
// All types now come from aura-app (which re-exports from aura-agent internally)
pub use aura_app::{
    BlockDelta, BlockReduction, ChatReduction, FactSource, FactStreamAdapter, GuardianDelta,
    GuardianReduction, InvitationReduction, ReactiveScheduler, RecoveryDelta, RecoveryReduction,
    SchedulerConfig, ViewAdapter,
};

// Re-export domain deltas
pub use aura_chat::ChatDelta;
pub use aura_invitation::InvitationDelta;
