//! # Core Application Module
//!
//! This module contains the core application types and logic:
//!
//! - [`AppCore`]: The main application entry point
//! - [`Intent`]: User actions that become facts
//! - [`StateSnapshot`]: FFI-safe state snapshot
//! - [`AppConfig`]: Application configuration
//! - [`IntentError`]: Error types for intent dispatch
//!
//! Note: The legacy "string fact" reducer pipeline has been removed. Journal facts are committed
//! by the runtime and delivered to UIs via typed reactive signals.

mod app;
mod error;
mod intent;
mod snapshot;

pub use app::{AppConfig, AppCore};
pub use error::IntentError;
pub use intent::{ChannelType as IntentChannelType, Intent, InvitationType, Screen};
pub use snapshot::StateSnapshot;
