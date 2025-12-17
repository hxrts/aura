//! # Settings Screen Module
//!
//! Account settings with profile, threshold, devices, and MFA configuration.

mod add_device_modal;
mod nickname_modal;
mod remove_device_modal;
mod screen;

// Screen exports
pub use screen::{run_settings_screen, MfaCallback, SettingsScreen};
