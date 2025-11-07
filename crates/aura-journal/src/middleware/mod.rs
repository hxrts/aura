//! Journal-specific middleware system
//!
//! This module provides middleware for journal operations including:
//! - Observability (metrics and tracing)
//! - Authorization (operation validation)
//! - Audit logging (compliance tracking)
//! - Caching (performance optimization)
//! - Input validation
//! - Retry logic
//! - Rate limiting

pub mod audit;
pub mod authorization;
pub mod caching;
pub mod handler;
pub mod integration;
pub mod observability;
pub mod rate_limiting;
pub mod retry;
pub mod stack;
pub mod validation;

pub use audit::*;
pub use authorization::*;
pub use caching::*;
pub use handler::*;
pub use integration::*;
pub use observability::*;
pub use rate_limiting::*;
pub use retry::*;
pub use stack::*;
pub use validation::*;

use crate::error::Result;
use crate::operations::JournalOperation;
use aura_types::{AccountId, DeviceId};

/// Context for journal middleware operations
#[derive(Debug, Clone)]
pub struct JournalContext {
    /// Account being operated on
    pub account_id: AccountId,

    /// Device performing the operation
    pub device_id: DeviceId,

    /// Operation being performed
    pub operation_type: String,

    /// Request timestamp
    pub timestamp: u64,

    /// Additional metadata
    pub metadata: std::collections::HashMap<String, String>,
}

impl JournalContext {
    /// Create a new journal context with the given timestamp
    pub fn new(
        account_id: AccountId,
        device_id: DeviceId,
        operation_type: String,
        timestamp: u64,
    ) -> Self {
        Self {
            account_id,
            device_id,
            operation_type,
            timestamp,
            metadata: std::collections::HashMap::new(),
        }
    }

    /// Create a new journal context with current system time
    ///
    /// Note: For testable code, use `new()` with a timestamp from TimeEffects instead
    #[allow(clippy::disallowed_methods)]
    pub fn new_with_system_time(
        account_id: AccountId,
        device_id: DeviceId,
        operation_type: String,
    ) -> Self {
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        Self::new(account_id, device_id, operation_type, timestamp)
    }

    /// Add metadata to the context
    pub fn with_metadata(mut self, key: String, value: String) -> Self {
        self.metadata.insert(key, value);
        self
    }
}

/// Trait for journal middleware components
pub trait JournalMiddleware: Send + Sync {
    /// Process a journal operation
    fn process(
        &self,
        operation: JournalOperation,
        context: &JournalContext,
        next: &dyn JournalHandler,
    ) -> Result<serde_json::Value>;

    /// Get middleware name for debugging
    fn name(&self) -> &str;
}

/// Trait for handling journal operations
pub trait JournalHandler: Send + Sync {
    /// Handle a journal operation
    fn handle(
        &self,
        operation: JournalOperation,
        context: &JournalContext,
    ) -> Result<serde_json::Value>;
}
