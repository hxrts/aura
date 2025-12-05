//! # aura-app: Unified Frontend Interface
//!
//! This crate provides the portable, platform-agnostic application core for Aura.
//! It is the **only** interface that frontends should use - TUI, CLI, iOS, Android,
//! and web applications all access the Aura runtime through `AppCore`.
//!
//! ## Architecture
//!
//! `AppCore` wraps `AuraAgent` to provide a clean API that hides the complexity
//! of the effect system from UI code:
//!
//! ```text
//! ┌─────────────┐  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐
//! │     TUI     │  │     CLI     │  │     iOS     │  │     Web     │
//! └──────┬──────┘  └──────┬──────┘  └──────┬──────┘  └──────┬──────┘
//!        │                │                │                │
//!        └────────────────┴────────────────┴────────────────┘
//!                                │
//!                                ↓
//!                    ┌───────────────────────┐
//!                    │       AppCore         │  ← aura-app (THIS CRATE)
//!                    │                       │
//!                    │  • ViewState signals  │
//!                    │  • Intent dispatch    │
//!                    │  • Service wrappers   │
//!                    └───────────┬───────────┘
//!                                │
//!                                ↓ (internal, hidden from frontends)
//!                    ┌───────────────────────┐
//!                    │      AuraAgent        │  ← aura-agent (runtime)
//!                    └───────────────────────┘
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
//! use aura_app::{AppCore, AppConfig, AgentBuilder};
//!
//! // Demo/Offline mode - local state only
//! let app = AppCore::new(config)?;
//!
//! // Production mode - with agent for full functionality
//! let agent = AgentBuilder::new()
//!     .with_authority(authority_id)
//!     .build_production()
//!     .await?;
//! let app = AppCore::with_agent(config, agent)?;
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
//! // Access agent when available (production mode)
//! if app.has_agent() {
//!     let agent = app.agent().unwrap();
//!     // Use agent services...
//! }
//!
//! // Subscribe to state changes
//! #[cfg(feature = "signals")]
//! let chat_signal = app.chat_signal();
//! ```
//!
//! ## Re-exports
//!
//! This crate re-exports types from `aura-agent` so frontends don't need
//! direct dependencies on internal crates. Import everything from `aura_app`.

// =============================================================================
// UniFFI scaffolding (when building for mobile)
// =============================================================================

#[cfg(feature = "uniffi")]
uniffi::setup_scaffolding!();

// =============================================================================
// Modules
// =============================================================================

pub mod bridge;
pub mod core;
pub mod queries;
pub mod views;

#[cfg(feature = "signals")]
pub mod signals;

pub mod platform;

// =============================================================================
// Re-exports
// =============================================================================

pub use crate::core::{
    AppConfig, AppCore, Intent, IntentChannelType, IntentError, InvitationType, Screen,
    StateSnapshot,
};
pub use crate::queries::Query;
pub use crate::views::{
    BlockState, Channel, ChannelType, ChatState, ContactsState, InvitationsState, Message,
    NeighborhoodState, RecoveryState, ViewState,
};

#[cfg(feature = "callbacks")]
pub use crate::bridge::callback::StateObserver;

#[cfg(feature = "signals")]
pub use crate::signals::{ReactiveState, ReactiveVec};

// Re-export commonly used types from aura-core
pub use aura_core::identifiers::{AuthorityId, ContextId};
pub use aura_core::time::TimeStamp;

// Re-export agent types for frontends (so they don't need to import from aura-agent)
pub use aura_agent::{AgentBuilder, AgentConfig, AuraAgent, AuraEffectSystem, EffectContext};

// Re-export configuration types
pub use aura_agent::core::config::StorageConfig;

// Re-export service types needed by AppCore methods
// Note: Some types are aliased to avoid conflicts with app-layer types
pub use aura_agent::{
    // Auth types
    AuthChallenge,
    AuthMethod,
    AuthResponse,
    AuthResult,
    // Recovery types (use agent:: prefix for RecoveryState to avoid conflict)
    GuardianApproval,
    // Invitation types (use agent:: prefix for InvitationType to avoid conflict)
    Invitation,
    InvitationResult,
    InvitationStatus,
    InvitationType as AgentInvitationType,
    RecoveryResult,
    RecoveryState as AgentRecoveryState,
    // Sync types
    SyncManagerConfig,
    SyncServiceManager,
};

// Re-export reactive types for TUI/signals integration
pub use aura_agent::reactive::{Dynamic, FactSource, ReactiveScheduler};

// Re-export additional reactive types for reactive TUI integration
// These are used by aura-terminal's journal_bridge and test harnesses
pub use aura_agent::reactive::{
    BlockDelta, BlockReduction, ChatReduction, FactStreamAdapter, GuardianDelta, GuardianReduction,
    InvitationReduction, RecoveryDelta, RecoveryReduction, SchedulerConfig, ViewAdapter,
};
