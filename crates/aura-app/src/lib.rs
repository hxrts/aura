//! # aura-app: Pure Application Core
//!
//! This crate provides the portable, platform-agnostic application core for Aura.
//! It contains pure business logic (intents, reducers, views) without runtime dependencies.
//!
//! ## Architecture
//!
//! `aura-app` is pure - it defines the application logic without runtime dependencies.
//! The `RuntimeBridge` trait enables dependency inversion: `aura-agent` implements
//! `RuntimeBridge` and depends on `aura-app`, not vice versa.
//!
//! ```text
//! ┌─────────────┐  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐
//! │     TUI     │  │     CLI     │  │     iOS     │  │     Web     │
//! └──────┬──────┘  └──────┬──────┘  └──────┬──────┘  └──────┬──────┘
//!        │                │                │                │
//!        └────────────────┴────────────────┴────────────────┘
//!                                │
//!        ┌───────────────────────┼───────────────────────┐
//!        ↓                       ↓                       ↓
//! ┌─────────────┐       ┌─────────────┐       ┌─────────────┐
//! │  aura-app   │       │ aura-agent  │       │   mocks     │
//! │  (pure)     │←──────│ (runtime)   │       │   (test)    │
//! │             │       │ implements  │       │ implements  │
//! │ RuntimeBridge trait │ RuntimeBridge       │ RuntimeBridge
//! └─────────────┘       └─────────────┘       └─────────────┘
//! ```
//!
//! ## Push-Based Reactive Flow
//!
//! All state changes flow through facts:
//!
//! ```text
//! Intent → Authorize (Biscuit) → Journal → Reduce → ViewState → Signal → UI
//! ```
//!
//! - **Intents**: User actions that become facts in the journal
//! - **Views**: Derived state computed by reducing facts
//! - **Signals**: Push-based notifications to UI (no polling)
//!
//! ## Construction Modes
//!
//! ```rust,ignore
//! use aura_app::{AppCore, AppConfig, RuntimeBridge};
//! use aura_agent::{AgentBuilder, AuraAgent};  // From aura-agent
//!
//! // Demo/Offline mode - local state only
//! let app = AppCore::new(config)?;
//!
//! // Production mode - with runtime bridge for full functionality
//! let agent = AgentBuilder::new()
//!     .with_authority(authority_id)
//!     .build_production()
//!     .await?;
//! let app = AppCore::with_runtime(config, agent.as_runtime_bridge())?;
//! ```
//!
//! ## Features
//!
//! - `native`: Enable futures-signals API for Rust consumers
//! - `ios`: Enable UniFFI bindings for iOS/Swift
//! - `android`: Enable UniFFI bindings for Android/Kotlin
//! - `web-js`: Enable wasm-bindgen for JavaScript consumers
//! - `web-dominator`: Enable dominator/signals for Rust WASM apps
//!
//! ## Usage
//!
//! ```rust,ignore
//! use aura_app::{AppCore, Intent};
//!
//! // Dispatch an intent (becomes a fact)
//! app.dispatch(Intent::SendMessage {
//!     channel_id,
//!     content: "Hello!".into(),
//!     reply_to: None,
//! })?;
//!
//! // Check runtime status
//! if app.has_runtime() {
//!     let sync_status = app.is_sync_running().await;
//! }
//!
//! // Subscribe to state changes
//! #[cfg(feature = "signals")]
//! let chat_signal = app.chat_signal();
//! ```
//!
//! ## Import Guide
//!
//! Frontends should import from both crates:
//! - **From `aura_app`**: `AppCore`, `Intent`, `ViewState`, `RuntimeBridge`
//! - **From `aura_agent`**: `AuraAgent`, `AgentBuilder`, services, reactive types

// =============================================================================
// UniFFI scaffolding (when building for mobile)
// =============================================================================

#![allow(unpredictable_function_pointer_comparisons)]

#[cfg(feature = "uniffi")]
uniffi::setup_scaffolding!();

// UniFFI custom type bridge for ContextId (string-based representation)
#[cfg(feature = "uniffi")]
impl crate::UniffiCustomTypeConverter for ContextId {
    type Builtin = String;

    fn into_custom(val: Self::Builtin) -> uniffi::Result<Self> {
        val.parse().map_err(uniffi::deps::anyhow::Error::new)
    }

    fn from_custom(obj: Self) -> Self::Builtin {
        obj.to_string()
    }
}

#[cfg(feature = "uniffi")]
uniffi::custom_type!(ContextId, String);

// =============================================================================
// Modules
// =============================================================================

pub mod bridge;
pub mod budget;
pub mod core;
pub mod queries;
pub mod runtime_bridge;
pub mod signal_defs;
pub mod views;
pub mod workflows;

#[cfg(feature = "signals")]
pub mod reactive_state;

pub mod platform;

// =============================================================================
// Re-exports
// =============================================================================

pub use crate::core::{
    AppConfig, AppCore, Intent, IntentChannelType, IntentError, InvitationType, Screen,
    StateSnapshot,
};

// Runtime bridge (for dependency inversion)
pub use crate::queries::Query;
pub use crate::runtime_bridge::{
    BoxedRuntimeBridge, LanPeerInfo, OfflineRuntimeBridge, RendezvousStatus, RuntimeBridge,
    RuntimeStatus, SyncStatus,
};
pub use crate::views::{
    BlockState, Channel, ChannelType, ChatState, ContactsState, InvitationsState, Message,
    NeighborhoodState, RecoveryState, ViewState,
};

#[cfg(feature = "callbacks")]
pub use crate::bridge::callback::StateObserver;

#[cfg(feature = "signals")]
pub use crate::reactive_state::{ReactiveState, ReactiveVec};

// Re-export budget types
pub use crate::budget::{
    BlockFlowBudget, BudgetBreakdown, BudgetError, BLOCK_TOTAL_SIZE, KB, MAX_NEIGHBORHOODS,
    MAX_RESIDENTS, MB, NEIGHBORHOOD_DONATION, RESIDENT_ALLOCATION,
};

// Re-export signal definitions for convenience
// Note: SyncStatus and ConnectionStatus are signal-specific types in signal_defs module.
// The runtime_bridge::SyncStatus is different (runtime status).
pub use crate::signal_defs::{
    register_app_signals, AppError, BLOCKS_SIGNAL, BLOCK_SIGNAL, BUDGET_SIGNAL, CHAT_SIGNAL,
    CONNECTION_STATUS_SIGNAL, CONTACTS_SIGNAL, ERROR_SIGNAL, INVITATIONS_SIGNAL,
    NEIGHBORHOOD_SIGNAL, RECOVERY_SIGNAL, SYNC_STATUS_SIGNAL, UNREAD_COUNT_SIGNAL,
};
// For signal-specific types, use the full path:
// - signal_defs::ConnectionStatus (signal value type)
// - signal_defs::SyncStatus (signal value type - different from runtime_bridge::SyncStatus)

// Re-export commonly used types from aura-core
pub use aura_core::identifiers::{AuthorityId, ContextId};
pub use aura_core::time::TimeStamp;

// Note: Agent types (AuraAgent, AgentBuilder, reactive types, services) are NOT
// re-exported here. With the dependency inversion:
// - aura-app is pure (no aura-agent dependency)
// - aura-agent depends on aura-app and implements RuntimeBridge
// - Frontends import app types from aura_app, runtime types from aura_agent
//
// Example frontend imports:
//   use aura_app::{AppCore, Intent, ViewState, RuntimeBridge};
//   use aura_agent::{AuraAgent, AgentBuilder, EffectContext};
//   use aura_agent::reactive::{Dynamic, ReactiveScheduler};
