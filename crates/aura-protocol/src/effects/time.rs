//! Time effects trait definitions
//!
//! This module defines the trait interface for time operations.
//! Implementations are provided in aura-protocol crate.

use std::time::Duration;
use aura_types::AuraError;
use async_trait::async_trait;

/// Wake conditions for cooperative yielding
#[derive(Debug, Clone)]
pub enum WakeCondition {
    /// Wake when new events are available
    NewEvents,
    /// Wake when a specific epoch/timestamp is reached
    EpochReached(u64),
    /// Wake after a timeout at specific timestamp
    TimeoutAt(u64),
    /// Wake when an event matching criteria is received
    EventMatching(String),
    /// Wake when threshold number of events received
    ThresholdEvents {
        /// Number of events to wait for before waking
        threshold: usize,
        /// Maximum time to wait in milliseconds
        timeout_ms: u64,
    },
}

/// Time effects interface
///
/// This trait provides time operations for the Aura effects system.
/// Implementations in aura-protocol provide:
/// - Production: Real system time operations
/// - Testing: Controllable time for deterministic tests
/// - Simulation: Time acceleration and scenarios
#[async_trait]
pub trait TimeEffects: Send + Sync {
    /// Get the current timestamp in epoch seconds
    async fn current_timestamp(&self) -> u64;

    /// Get current timestamp in milliseconds
    async fn current_timestamp_millis(&self) -> u64;

    /// Delay execution for a specified duration
    async fn delay(&self, duration: Duration);

    /// Yield execution until a condition is met
    async fn yield_until(&self, condition: WakeCondition) -> Result<(), aura_types::AuraError>;
}