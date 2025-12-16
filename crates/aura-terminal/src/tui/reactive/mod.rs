//! # TUI Reactive Module
//!
//! Reactive data bindings for the TUI. This module re-exports view types from
//! `aura-app` and reactive infrastructure from `aura-agent`.
//!
//! ## Architecture Note
//!
//! Query types are defined in `aura_app::queries` and implement `aura_core::Query`.
//! View types (Channel, Message, Guardian, etc.) come from `aura_app::views`.
//! FRP primitives (Dynamic<T>) and journal integration are in `aura-agent::reactive`.
//!
//! The reactive pipeline is:
//! - AppCore signals emit updates when facts change
//! - TUI components subscribe via `use_future` and `ReactiveEffects::subscribe`
//! - Views render from aura_app view types
//!
//! ## Journal-Reactive Bridge
//!
//! The journal → view flow is handled by:
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
//! 4. **TuiContext** (`crate::tui::context::TuiContext`)
//!    - Wires up all components on construction
//!    - Spawns background tasks for fact forwarding
//!    - Provides `fact_stream_adapter()` for feeding facts
//!
//! ## Integration Points
//!
//! - `aura_app::queries::*` - Portable Datalog queries
//! - `aura_app::views::*` - Canonical view types (Channel, Message, Guardian, etc.)
//! - `aura_core::effects::reactive::ReactiveEffects` - Signal subscription
//! - `crate::tui::context::IoContext` - Effect dispatch

// Re-export shared FRP primitives from aura-agent (runtime layer)
pub use aura_agent::reactive::Dynamic;
pub use aura_agent::reactive::ReactiveScheduler;

// Re-export reactive infrastructure types from aura-agent
pub use aura_agent::reactive::{
    BlockDelta, BlockReduction, ChatReduction, FactSource, FactStreamAdapter, GuardianDelta,
    GuardianReduction, InvitationReduction, RecoveryDelta, RecoveryReduction, SchedulerConfig,
    ViewAdapter,
};

// Re-export domain deltas
pub use aura_chat::ChatDelta;
pub use aura_invitation::InvitationDelta;

// Re-export view types from aura-app
pub use aura_app::views::block::{BlockState, Resident, ResidentRole, StorageBudget};
pub use aura_app::views::chat::{Channel, ChannelType, Message};
pub use aura_app::views::contacts::{Contact, MySuggestion, SuggestionPolicy};
pub use aura_app::views::invitations::{
    Invitation, InvitationDirection, InvitationStatus, InvitationType,
};
pub use aura_app::views::neighborhood::{AdjacencyType, NeighborBlock, TraversalPosition};
pub use aura_app::views::recovery::{
    Guardian, GuardianStatus, RecoveryApproval as GuardianApproval,
    RecoveryProcessStatus as RecoveryStatus, RecoveryState,
};

// Re-export query types from aura-app for convenience
pub use aura_app::queries::{
    ChannelsQuery, ContactsQuery, GuardiansQuery, InvitationsQuery, MessagesQuery, RecoveryQuery,
};

// Re-export ThresholdConfig from aura-core
pub use aura_core::threshold::ThresholdConfig;
