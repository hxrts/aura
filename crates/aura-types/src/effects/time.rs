//! Time effects trait definitions
//!
//! This module defines the trait interface for time operations.
//! Implementations are provided in aura-protocol and aura-crypto crates.

use crate::AuraError;

/// Time effects interface for deterministic time operations
///
/// This trait provides time operations for the Aura effects system.
/// Implementations provide:
/// - Production: Real system time operations
/// - Testing: Controllable/deterministic time for tests
/// - Simulation: Time acceleration and scenarios
pub trait TimeEffects: Send + Sync {
    /// Get the current timestamp in epoch seconds
    fn now(&self) -> Result<u64, AuraError>;

    /// Get current timestamp in milliseconds
    fn now_millis(&self) -> Result<u64, AuraError> {
        self.now().map(|secs| secs * 1000)
    }

    /// Advance time by the given number of seconds (for testing)
    fn advance_time(&self, seconds: u64) -> Result<(), AuraError>;
}

/// Simple system time effects using platform time
///
/// This is a basic implementation that delegates to system time utilities.
/// TODO: this needs to be removed and fully replaced with the injectable implementation from aura-protocol.
#[derive(Debug, Clone, Copy)]
pub struct SystemTimeEffects;

impl SystemTimeEffects {
    /// Create a new system time effects instance
    pub fn new() -> Self {
        Self
    }
}

impl Default for SystemTimeEffects {
    fn default() -> Self {
        Self::new()
    }
}

impl TimeEffects for SystemTimeEffects {
    fn now(&self) -> Result<u64, AuraError> {
        Ok(crate::time::current_unix_timestamp())
    }

    fn now_millis(&self) -> Result<u64, AuraError> {
        Ok(crate::time::current_unix_timestamp_millis())
    }

    fn advance_time(&self, _seconds: u64) -> Result<(), AuraError> {
        // No-op for system time
        Ok(())
    }
}
