//! # Reactive Bridges
//!
//! This module provides bridges between the application core and
//! platform-specific reactive systems:
//!
//! - **callbacks**: Callback-based API for UniFFI (iOS/Android)
//! - **signals**: Signal-based API for native Rust/dominator

#[cfg(feature = "callbacks")]
pub mod callback;

#[cfg(feature = "signals")]
pub mod signals;
