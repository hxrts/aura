//! # Platform-Specific Helpers
//!
//! This module contains platform-specific initialization and helpers.

#[cfg(any(feature = "ios", test))]
pub mod ios;

#[cfg(any(feature = "android", test))]
pub mod android;

#[cfg(feature = "wasm")]
pub mod wasm;
