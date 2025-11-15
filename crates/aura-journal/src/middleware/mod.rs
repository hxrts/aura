//! Journal operation context types
//!
//! **MIGRATION NOTE**: Middleware patterns removed - migrated to effect system
//! This module now contains only essential context types for journal operations.
//! All middleware functionality has been moved to the unified effect system.

use aura_core::{AccountId, DeviceId};

/// Context for journal operations
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
            .map(|d| d.as_secs())
            .unwrap_or(0);
        Self::new(account_id, device_id, operation_type, timestamp)
    }

    /// Add metadata to the context
    pub fn with_metadata(mut self, key: String, value: String) -> Self {
        self.metadata.insert(key, value);
        self
    }
}

// Middleware patterns removed - migrated to AuthorizationEffects, ReliabilityEffects, etc.
// TODO: Complete migration by implementing proper effect handlers in aura-effects
