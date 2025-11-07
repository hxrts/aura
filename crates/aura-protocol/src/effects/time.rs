//! Time effects trait definitions
//!
//! This module defines the trait interface for time operations.
//! Implementations are provided in aura-protocol crate.

use async_trait::async_trait;
use aura_types::AuraError;
use std::time::Duration;
use uuid::Uuid;

/// Wake conditions for cooperative yielding
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
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
    /// Wake immediately (no wait)
    Immediate,
    /// Custom wake condition with arbitrary string
    Custom(String),
}

/// Time operation errors
#[derive(Debug, thiserror::Error)]
pub enum TimeError {
    /// Invalid epoch timestamp provided
    #[error("Invalid epoch: {epoch}")]
    InvalidEpoch { epoch: u64 },
    /// Operation timed out
    #[error("Timeout after {timeout_ms}ms")]
    Timeout { timeout_ms: u64 },
    /// Clock synchronization failed
    #[error("Clock sync failed: {reason}")]
    ClockSyncFailed { reason: String },
    /// Time service unavailable
    #[error("Time service unavailable")]
    ServiceUnavailable,
}

/// Handle for timeout operations
pub type TimeoutHandle = Uuid;

/// Time effects interface
///
/// This trait provides time operations for the Aura effects system.
/// Implementations in aura-protocol provide:
/// - Production: Real system time operations
/// - Testing: Controllable time for deterministic tests
/// - Simulation: Time acceleration and scenarios
#[async_trait]
pub trait TimeEffects: Send + Sync {
    /// Get the current timestamp in epoch milliseconds
    async fn current_epoch(&self) -> u64;

    /// Get current timestamp in seconds
    async fn current_timestamp(&self) -> u64;

    /// Get current timestamp in milliseconds
    async fn current_timestamp_millis(&self) -> u64;

    /// Sleep for a specified number of milliseconds
    async fn sleep_ms(&self, ms: u64);

    /// Sleep until a specific epoch timestamp
    async fn sleep_until(&self, epoch: u64);

    /// Delay execution for a specified duration
    async fn delay(&self, duration: Duration);

    /// Sleep for specified duration in milliseconds
    async fn sleep(&self, duration_ms: u64) -> Result<(), AuraError>;

    /// Yield execution until a condition is met
    async fn yield_until(&self, condition: WakeCondition) -> Result<(), TimeError>;

    /// Wait until a condition is met (alias for yield_until with AuraError)
    async fn wait_until(&self, condition: WakeCondition) -> Result<(), AuraError>;

    /// Set a timeout and return a handle
    async fn set_timeout(&self, timeout_ms: u64) -> TimeoutHandle;

    /// Cancel a timeout by handle
    async fn cancel_timeout(&self, handle: TimeoutHandle) -> Result<(), TimeError>;

    /// Execute a future with timeout
    ///
    /// Note: This method is removed to make TimeEffects dyn-compatible.
    /// Use tokio::time::timeout directly in your code instead.
    // async fn timeout<F, T>(&self, future: F, duration_ms: u64) -> Result<T, AuraError>
    // where
    //     F: std::future::Future<Output = T> + Send + 'async_trait,
    //     T: Send + 'async_trait;

    /// Check if this is a simulated time handler
    fn is_simulated(&self) -> bool;

    /// Register a context for time events
    fn register_context(&self, context_id: Uuid);

    /// Unregister a context from time events
    fn unregister_context(&self, context_id: Uuid);

    /// Notify that events are available for waiting contexts
    async fn notify_events_available(&self);

    /// Get time resolution in milliseconds
    fn resolution_ms(&self) -> u64;
}
