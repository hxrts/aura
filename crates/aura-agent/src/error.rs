//! Agent errors - using unified error system
//!
//! This module provides agent error handling with the unified Aura error system,
//! including convenient extension traits for adding context to errors.

// Re-export unified error system
pub use aura_types::{AuraError, AuraResult as Result, ErrorCode, ErrorSeverity};

// Type aliases for backward compatibility
pub type AgentError = AuraError;
pub type ProtocolError = AuraError;
pub type CryptoError = AuraError;
pub type DataError = AuraError;
pub type InfrastructureError = AuraError;
pub type CapabilityError = AuraError;
pub type SystemError = AuraError;

// Agent-specific error constructors are provided by the aura-errors crate
// The AgentError type alias points to AuraError which has all the constructors

/// Extension trait for adding context to errors
pub trait ResultExt<T> {
    /// Add storage operation context
    fn storage_context(self, msg: &str) -> Result<T>;

    /// Add coordination operation context
    fn coord_context(self, msg: &str) -> Result<T>;

    /// Add configuration operation context
    fn config_context(self, msg: &str) -> Result<T>;

    /// Add serialization operation context
    fn serialize_context(self, msg: &str) -> Result<T>;

    /// Add deserialization operation context
    fn deserialize_context(self, msg: &str) -> Result<T>;
}

impl<T, E: std::fmt::Display> ResultExt<T> for std::result::Result<T, E> {
    fn storage_context(self, msg: &str) -> Result<T> {
        self.map_err(|e| AuraError::storage_failed(format!("{}: {}", msg, e)))
    }

    fn coord_context(self, msg: &str) -> Result<T> {
        self.map_err(|e| AuraError::coordination_failed(format!("{}: {}", msg, e)))
    }

    fn config_context(self, msg: &str) -> Result<T> {
        self.map_err(|e| AuraError::configuration_error(format!("{}: {}", msg, e)))
    }

    fn serialize_context(self, msg: &str) -> Result<T> {
        self.map_err(|e| AuraError::serialization_failed(format!("{}: {}", msg, e)))
    }

    fn deserialize_context(self, msg: &str) -> Result<T> {
        self.map_err(|e| AuraError::deserialization_failed(format!("{}: {}", msg, e)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_storage_context() {
        let result: std::result::Result<(), String> = Err("disk full".to_string());
        let error = result.storage_context("Failed to write").unwrap_err();
        let error_msg = format!("{}", error);
        assert!(error_msg.contains("Failed to write"));
        assert!(error_msg.contains("disk full"));
    }

    #[test]
    fn test_coord_context() {
        let result: std::result::Result<(), String> = Err("timeout".to_string());
        let error = result.coord_context("Session failed").unwrap_err();
        let error_msg = format!("{}", error);
        assert!(error_msg.contains("Session failed"));
        assert!(error_msg.contains("timeout"));
    }
}
