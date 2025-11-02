//! Time utility functions for the Aura codebase
//!
//! This module provides centralized time functions that can be easily
//! replaced with injectable time sources for testing and deterministic execution.

use std::time::{SystemTime, UNIX_EPOCH};

/// Get current Unix timestamp in seconds
/// 
/// TODO: Replace with injected time source in production
#[allow(clippy::disallowed_methods)]
pub fn current_unix_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Get current Unix timestamp in milliseconds
/// 
/// TODO: Replace with injected time source in production  
#[allow(clippy::disallowed_methods)]
pub fn current_unix_timestamp_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

/// Get current SystemTime
/// 
/// TODO: Replace with injected time source in production
#[allow(clippy::disallowed_methods)]
pub fn current_system_time() -> SystemTime {
    SystemTime::now()
}
