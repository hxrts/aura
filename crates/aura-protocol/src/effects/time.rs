//! Time effects interface
//!
//! Pure trait definitions for time and scheduling operations used by protocols.

use async_trait::async_trait;
use uuid::Uuid;

/// Time effects for protocol coordination and scheduling
#[async_trait]
pub trait TimeEffects: Send + Sync {
    /// Get the current epoch/timestamp
    async fn current_epoch(&self) -> u64;
    
    /// Sleep for the specified duration in milliseconds
    async fn sleep_ms(&self, ms: u64);
    
    /// Sleep until a specific epoch/timestamp
    async fn sleep_until(&self, epoch: u64);
    
    /// Yield execution until a condition is met
    async fn yield_until(&self, condition: WakeCondition) -> Result<(), TimeError>;
    
    /// Set a timeout for an operation
    async fn set_timeout(&self, timeout_ms: u64) -> TimeoutHandle;
    
    /// Cancel a previously set timeout
    async fn cancel_timeout(&self, handle: TimeoutHandle) -> Result<(), TimeError>;
    
    /// Check if we're running in simulated time mode
    fn is_simulated(&self) -> bool;
    
    /// Register a context for notifications
    fn register_context(&self, context_id: Uuid);
    
    /// Unregister a context
    fn unregister_context(&self, context_id: Uuid);
    
    /// Notify that events are available for waiting contexts
    async fn notify_events_available(&self);
    
    /// Get time resolution in milliseconds
    fn resolution_ms(&self) -> u64;
}

/// Wake conditions for cooperative yielding in protocol execution
#[derive(Debug, Clone)]
pub enum WakeCondition {
    /// Wake when new events are available for this context
    NewEvents,
    /// Wake when simulated time reaches this epoch
    EpochReached(u64),
    /// Wake when timeout expires
    TimeoutAt(u64),
    /// Wake when specific condition is met
    Custom(String),
    /// Wake immediately (for testing)
    Immediate,
}

/// Handle for a timeout operation
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TimeoutHandle(pub Uuid);

impl TimeoutHandle {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

/// Time-related errors
#[derive(Debug, thiserror::Error)]
pub enum TimeError {
    #[error("Operation timed out after {timeout_ms}ms")]
    Timeout { timeout_ms: u64 },
    
    #[error("Timeout handle not found: {handle:?}")]
    TimeoutNotFound { handle: TimeoutHandle },
    
    #[error("Invalid epoch: {epoch}")]
    InvalidEpoch { epoch: u64 },
    
    #[error("Time source not available")]
    TimeSourceNotAvailable,
    
    #[error("Clock skew detected: expected {expected}, got {actual}")]
    ClockSkew { expected: u64, actual: u64 },
    
    #[error("Time operation failed: {reason}")]
    OperationFailed { reason: String },
}