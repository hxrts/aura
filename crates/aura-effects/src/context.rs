//! Context propagation handlers
//!
//! This module provides stateless handlers for explicit context propagation.
//! These handlers replace middleware-based ambient context with explicit
//! parameter passing following Layer 3 principles.
//!
//! # Key Characteristics
//!
//! - **Stateless**: Context is passed explicitly, no ambient state
//! - **Single-party**: Context for one operation/device at a time
//! - **Context-free**: No assumptions about execution environment
//!
//! Note: This module may use `SystemTime::now()` in effect handler implementations
//! for timestamp generation. This is legitimate as it's in the effect handler layer.

// Allow disallowed methods in effect handler implementations
#![allow(clippy::disallowed_methods)]

//! # Usage
//!
//! ```rust,ignore
//! use aura_effects::context::StandardContextHandler;
//! use aura_core::DeviceId;
//!
//! // Create context with explicit parameters
//! let context = StandardContextHandler::new()
//!     .create_execution_context(device_id, operation_type);
//!
//! // Use context explicitly in operations
//! let result = handler.perform_operation(&context, &params).await?;
//! ```

use aura_core::identifiers::DeviceId;
use aura_core::time::PhysicalTime;
use aura_core::{AccountId, SessionId};
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

/// Execution context for operations
///
/// Replaces middleware-based ambient context with explicit parameter passing.
/// This context is passed explicitly to operations that need it.
#[derive(Debug, Clone)]
pub struct ExecutionContext {
    /// Device performing the operation
    pub device_id: DeviceId,

    /// Account being operated on (if applicable)
    pub account_id: Option<AccountId>,

    /// Session ID for the operation (if applicable)
    pub session_id: Option<SessionId>,

    /// Operation type being performed
    pub operation_type: String,

    /// Operation timestamp (uses unified time system)
    pub timestamp: PhysicalTime,

    /// Additional metadata
    pub metadata: HashMap<String, String>,
}

impl ExecutionContext {
    /// Create a new execution context
    pub fn new(device_id: DeviceId, operation_type: String) -> Self {
        let ts_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);

        Self {
            device_id,
            account_id: None,
            session_id: None,
            operation_type,
            timestamp: PhysicalTime {
                ts_ms,
                uncertainty: None,
            },
            metadata: HashMap::new(),
        }
    }

    /// Create context with explicit timestamp (for deterministic testing)
    pub fn with_timestamp(
        device_id: DeviceId,
        operation_type: String,
        timestamp: PhysicalTime,
    ) -> Self {
        Self {
            device_id,
            account_id: None,
            session_id: None,
            operation_type,
            timestamp,
            metadata: HashMap::new(),
        }
    }

    /// Create context with explicit timestamp in milliseconds (backward compatibility)
    pub fn with_timestamp_ms(
        device_id: DeviceId,
        operation_type: String,
        timestamp_ms: u64,
    ) -> Self {
        Self::with_timestamp(
            device_id,
            operation_type,
            PhysicalTime {
                ts_ms: timestamp_ms,
                uncertainty: None,
            },
        )
    }

    /// Get timestamp in milliseconds (backward compatibility)
    pub fn timestamp_ms(&self) -> u64 {
        self.timestamp.ts_ms
    }

    /// Set the account ID
    pub fn with_account(mut self, account_id: AccountId) -> Self {
        self.account_id = Some(account_id);
        self
    }

    /// Set the session ID
    pub fn with_session(mut self, session_id: SessionId) -> Self {
        self.session_id = Some(session_id);
        self
    }

    /// Add metadata entry
    pub fn with_metadata(mut self, key: String, value: String) -> Self {
        self.metadata.insert(key, value);
        self
    }

    /// Add multiple metadata entries
    pub fn with_metadata_map(mut self, metadata: HashMap<String, String>) -> Self {
        self.metadata.extend(metadata);
        self
    }

    /// Get metadata value
    pub fn get_metadata(&self, key: &str) -> Option<&String> {
        self.metadata.get(key)
    }

    /// Create a derived context for a sub-operation
    pub fn derive_for_operation(&self, operation_type: String) -> Self {
        Self {
            device_id: self.device_id,
            account_id: self.account_id,
            session_id: self.session_id,
            operation_type,
            timestamp: self.timestamp.clone(),
            metadata: self.metadata.clone(),
        }
    }
}

/// Standard context handler
///
/// Provides utilities for creating and managing execution contexts.
/// This is a stateless handler that follows Layer 3 principles.
#[derive(Debug, Clone)]
pub struct StandardContextHandler;

impl StandardContextHandler {
    /// Create a new context handler
    pub fn new() -> Self {
        Self
    }

    /// Create an execution context
    pub fn create_execution_context(
        &self,
        device_id: DeviceId,
        operation_type: String,
    ) -> ExecutionContext {
        ExecutionContext::new(device_id, operation_type)
    }

    /// Create context with explicit timestamp
    pub fn create_execution_context_with_timestamp(
        &self,
        device_id: DeviceId,
        operation_type: String,
        timestamp: PhysicalTime,
    ) -> ExecutionContext {
        ExecutionContext::with_timestamp(device_id, operation_type, timestamp)
    }

    /// Create context with explicit timestamp in milliseconds (backward compatibility)
    pub fn create_execution_context_with_timestamp_ms(
        &self,
        device_id: DeviceId,
        operation_type: String,
        timestamp_ms: u64,
    ) -> ExecutionContext {
        ExecutionContext::with_timestamp_ms(device_id, operation_type, timestamp_ms)
    }

    /// Validate context for operation
    pub fn validate_context(&self, context: &ExecutionContext, required_fields: &[&str]) -> bool {
        for field in required_fields {
            match *field {
                "account_id" => {
                    if context.account_id.is_none() {
                        return false;
                    }
                }
                "session_id" => {
                    if context.session_id.is_none() {
                        return false;
                    }
                }
                _ => {
                    if context.get_metadata(field).is_none() {
                        return false;
                    }
                }
            }
        }
        true
    }

    /// Merge metadata from multiple contexts
    pub fn merge_metadata(&self, contexts: &[&ExecutionContext]) -> HashMap<String, String> {
        let mut merged = HashMap::new();

        for context in contexts {
            for (key, value) in &context.metadata {
                merged.insert(key.clone(), value.clone());
            }
        }

        merged
    }
}

impl Default for StandardContextHandler {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::identifiers::DeviceId;
    use aura_core::{AccountId, SessionId};

    #[test]
    fn test_execution_context_creation() {
        let device_id = DeviceId::new();
        let operation_type = "test_operation".to_string();

        let context = ExecutionContext::new(device_id, operation_type.clone());

        assert_eq!(context.device_id, device_id);
        assert_eq!(context.operation_type, operation_type);
        assert!(context.account_id.is_none());
        assert!(context.session_id.is_none());
        assert!(context.timestamp_ms() > 0);
    }

    #[test]
    fn test_execution_context_builder() {
        let device_id = DeviceId::deterministic_test_id();
        let account_id = AccountId::new_from_entropy([1u8; 32]);
        let session_id = SessionId::from_uuid(uuid::Uuid::from_u128(1));

        let context = ExecutionContext::new(device_id, "test".to_string())
            .with_account(account_id)
            .with_session(session_id)
            .with_metadata("key1".to_string(), "value1".to_string())
            .with_metadata("key2".to_string(), "value2".to_string());

        assert_eq!(context.account_id, Some(account_id));
        assert_eq!(context.session_id, Some(session_id));
        assert_eq!(context.get_metadata("key1"), Some(&"value1".to_string()));
        assert_eq!(context.get_metadata("key2"), Some(&"value2".to_string()));
    }

    #[test]
    fn test_context_derivation() {
        let device_id = DeviceId::new_from_entropy([2u8; 32]);
        let account_id = AccountId::new_from_entropy([2u8; 32]);

        let original = ExecutionContext::new(device_id, "original".to_string())
            .with_account(account_id)
            .with_metadata("shared".to_string(), "data".to_string());

        let derived = original.derive_for_operation("derived".to_string());

        assert_eq!(derived.device_id, original.device_id);
        assert_eq!(derived.account_id, original.account_id);
        assert_eq!(derived.operation_type, "derived");
        assert_eq!(derived.timestamp, original.timestamp);
        assert_eq!(derived.get_metadata("shared"), Some(&"data".to_string()));
    }

    #[test]
    fn test_standard_context_handler() {
        let handler = StandardContextHandler::new();
        let device_id = DeviceId::new_from_entropy([3u8; 32]);

        let context = handler.create_execution_context(device_id, "test".to_string());

        assert_eq!(context.device_id, device_id);
        assert_eq!(context.operation_type, "test");
    }

    #[test]
    fn test_context_validation() {
        let handler = StandardContextHandler::new();
        let device_id = DeviceId::new_from_entropy([4u8; 32]);
        let account_id = AccountId::new_from_entropy([4u8; 32]);

        let context = ExecutionContext::new(device_id, "test".to_string())
            .with_account(account_id)
            .with_metadata("custom_field".to_string(), "value".to_string());

        // Should pass validation for available fields
        assert!(handler.validate_context(&context, &["account_id"]));
        assert!(handler.validate_context(&context, &["custom_field"]));
        assert!(handler.validate_context(&context, &["account_id", "custom_field"]));

        // Should fail validation for missing fields
        assert!(!handler.validate_context(&context, &["session_id"]));
        assert!(!handler.validate_context(&context, &["missing_field"]));
    }

    #[test]
    fn test_metadata_merging() {
        let handler = StandardContextHandler::new();
        let device_id = DeviceId::new_from_entropy([5u8; 32]);

        let context1 = ExecutionContext::new(device_id, "op1".to_string())
            .with_metadata("key1".to_string(), "value1".to_string())
            .with_metadata("shared".to_string(), "from_context1".to_string());

        let context2 = ExecutionContext::new(device_id, "op2".to_string())
            .with_metadata("key2".to_string(), "value2".to_string())
            .with_metadata("shared".to_string(), "from_context2".to_string());

        let merged = handler.merge_metadata(&[&context1, &context2]);

        assert_eq!(merged.get("key1"), Some(&"value1".to_string()));
        assert_eq!(merged.get("key2"), Some(&"value2".to_string()));
        // Later context should override earlier one for shared keys
        assert_eq!(merged.get("shared"), Some(&"from_context2".to_string()));
    }
}
