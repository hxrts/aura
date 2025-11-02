//! Store errors - using unified error system

// Re-export unified error system
pub use aura_types::{AuraError, AuraResult as Result, ErrorCode, ErrorContext, ErrorSeverity};

// Type aliases for backward compatibility
pub type StoreError = AuraError;

// Store-specific error constructors using builder pattern
pub struct StoreErrorBuilder;

impl StoreErrorBuilder {
    /// Create a quota exceeded error
    pub fn quota_exceeded(used: u64, limit: u64) -> AuraError {
        AuraError::Infrastructure(
            aura_types::errors::InfrastructureError::StorageQuotaExceeded {
                message: format!("Storage quota exceeded: used {}, limit {}", used, limit),
                context: "".to_string(),
            },
        )
    }

    /// Create a not found error
    pub fn not_found(resource: impl Into<String>) -> AuraError {
        AuraError::storage_read_failed(format!("Entry not found: {}", resource.into()))
    }

    /// Create an insufficient capacity error
    pub fn insufficient_capacity(needed: u64, available: u64) -> AuraError {
        AuraError::storage_failed(format!(
            "Insufficient capacity: need {}, available {}",
            needed, available
        ))
    }

    /// Create an authentication failed error
    pub fn authentication_failed(reason: impl Into<String>) -> AuraError {
        AuraError::transport_connection_failed(format!("Authentication failed: {}", reason.into()))
    }

    /// Create an insufficient permissions error
    pub fn insufficient_permissions_store(
        required: impl Into<String>,
        actual: impl Into<String>,
    ) -> AuraError {
        AuraError::insufficient_capability(format!(
            "Insufficient permissions: required {}, have {}",
            required.into(),
            actual.into()
        ))
    }

    /// Create a capability expired error
    pub fn capability_expired(expired_at: u64) -> AuraError {
        AuraError::insufficient_capability(format!("Capability expired at {}", expired_at))
    }

    /// Create a capability revoked error
    pub fn capability_revoked(capability_id: impl Into<String>) -> AuraError {
        AuraError::insufficient_capability(format!("Capability revoked: {}", capability_id.into()))
    }

    /// Create an access denied error
    pub fn access_denied(reason: impl Into<String>) -> AuraError {
        AuraError::insufficient_capability(format!("Access denied: {}", reason.into()))
    }

    /// Create an integrity check failed error
    pub fn integrity_check_failed(details: impl Into<String>) -> AuraError {
        AuraError::data_corruption_detected(format!("Integrity check failed: {}", details.into()))
    }

    /// Create a hash mismatch error
    pub fn hash_mismatch(expected: impl Into<String>, actual: impl Into<String>) -> AuraError {
        AuraError::data_corruption_detected(format!(
            "Content hash mismatch: expected {}, got {}",
            expected.into(),
            actual.into()
        ))
    }

    /// Create a Merkle verification failed error
    pub fn merkle_verification_failed(reason: impl Into<String>) -> AuraError {
        AuraError::data_corruption_detected(format!(
            "Merkle proof verification failed: {}",
            reason.into()
        ))
    }

    /// Create a network connection failed error
    pub fn network_connection_failed(reason: impl Into<String>) -> AuraError {
        AuraError::network_unreachable(format!("Network connection failed: {}", reason.into()))
    }

    /// Create a network timeout error
    pub fn network_timeout(operation: impl Into<String>) -> AuraError {
        AuraError::transport_timeout(format!("Network timeout: {}", operation.into()))
    }

    /// Create a peer unavailable error
    pub fn peer_unavailable(peer_id: impl Into<String>) -> AuraError {
        AuraError::network_unreachable(format!("Peer unavailable: {}", peer_id.into()))
    }

    /// Create a replication failed error
    pub fn replication_failed(reason: impl Into<String>) -> AuraError {
        AuraError::network_partition(format!("Replication failed: {}", reason.into()))
    }

    /// Create an invalid operation error
    pub fn invalid_operation_store(operation: impl Into<String>) -> AuraError {
        AuraError::operation_not_allowed(format!("Invalid operation: {}", operation.into()))
    }

    /// Create a key version mismatch error
    pub fn key_version_mismatch(expected: u32, actual: u32) -> AuraError {
        AuraError::data_corruption_detected(format!(
            "Key version mismatch: expected {}, got {}",
            expected, actual
        ))
    }

    /// Create a replay attack detected error
    pub fn replay_attack_detected(reason: impl Into<String>) -> AuraError {
        AuraError::data_corruption_detected(format!("Replay attack detected: {}", reason.into()))
    }

    /// Create an invalid configuration error
    pub fn invalid_configuration(field: impl Into<String>, reason: impl Into<String>) -> AuraError {
        AuraError::configuration_error(format!(
            "Invalid configuration: {} - {}",
            field.into(),
            reason.into()
        ))
    }

    /// Create an invalid protocol state error
    pub fn invalid_protocol_state(reason: impl Into<String>) -> AuraError {
        AuraError::coordination_failed(format!("Invalid protocol state: {}", reason.into()))
    }

    /// Create an IO error
    pub fn io_error(operation: impl Into<String>) -> AuraError {
        AuraError::storage_failed(format!("IO error: {}", operation.into()))
    }
}
