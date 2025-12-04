//! # aura-app: Portable Headless Application Core
//!
//! This crate provides the portable, platform-agnostic application core for Aura.
//! It can be used to build terminal UIs, iOS apps (via UniFFI), Android apps,
//! and web applications (via WASM/dominator).
//!
//! ## Architecture
//!
//! The application core follows a fact-based architecture:
//!
//! ```text
//! Intent → Authorize (Biscuit) → Journal → Reduce → View → Sync
//! ```
//!
//! - **Intents**: User actions that become facts in the journal
//! - **Views**: Derived state computed by reducing facts
//! - **Queries**: Typed wrappers that compile to Datalog
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
//! // Create the app core
//! let app = AppCore::new(config)?;
//!
//! // Dispatch an intent (becomes a fact)
//! app.dispatch(Intent::SendMessage {
//!     channel_id,
//!     content: "Hello!".into(),
//!     reply_to: None,
//! })?;
//!
//! // Subscribe to state changes (native/dominator)
//! #[cfg(feature = "signals")]
//! let chat_signal = app.chat_signal();
//!
//! // Subscribe via callbacks (UniFFI/mobile)
//! #[cfg(feature = "callbacks")]
//! app.subscribe(observer);
//! ```

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
