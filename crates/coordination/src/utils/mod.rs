//! Coordination utilities
//!
//! This module provides low-level utilities used by choreographic protocols:
//! - Event watching and filtering for CRDT synchronization
//! - Cryptographic signing utilities for protocol events
//! - Distributed lottery protocol for conflict resolution

pub mod event_watcher;
pub mod signing;
pub mod lottery;

pub use event_watcher::*;
pub use signing::*;
pub use lottery::*;