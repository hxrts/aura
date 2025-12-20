//! # Core Application Module
//!
//! This module contains the core application types and logic:
//!
//! - [`AppCore`]: The main application entry point
//! - [`Intent`]: User actions that become facts
//! - [`StateSnapshot`]: FFI-safe state snapshot
//! - [`AppConfig`]: Application configuration
//! - [`IntentError`]: Error types for intent dispatch
//! - [`ViewDelta`]: Changes to apply to view state
//! - [`reduce_fact`]: Convert journal facts to view deltas

mod app;
mod error;
mod intent;
mod reducer;
#[cfg(feature = "signals")]
mod signal_sync;
mod snapshot;

pub use app::{AppConfig, AppCore};
pub use error::IntentError;
pub use intent::{ChannelType as IntentChannelType, Intent, InvitationType, Screen};
pub use reducer::{reduce_fact, ViewDelta};
#[cfg(feature = "signals")]
pub use signal_sync::SignalForwarder;
pub use snapshot::StateSnapshot;
