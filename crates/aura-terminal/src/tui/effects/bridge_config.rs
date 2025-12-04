//! # Bridge Configuration
//!
//! Configuration and state types for the effect bridge.

use std::time::Duration;

/// Configuration for the effect bridge
#[derive(Debug, Clone)]
pub struct BridgeConfig {
    /// Maximum pending commands in the queue
    pub command_buffer_size: usize,
    /// Maximum pending events in the broadcast channel
    pub event_buffer_size: usize,
    /// Timeout for command execution
    pub command_timeout: Duration,
    /// Enable automatic retry on transient failures
    pub auto_retry: bool,
    /// Maximum retry attempts
    pub max_retries: u32,
    /// Backoff duration between retries
    pub retry_backoff: Duration,
}

impl Default for BridgeConfig {
    fn default() -> Self {
        Self {
            command_buffer_size: 256,
            event_buffer_size: 1024,
            command_timeout: Duration::from_secs(30),
            auto_retry: true,
            max_retries: 3,
            retry_backoff: Duration::from_millis(100),
        }
    }
}
