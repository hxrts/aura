//! Unified error handling for aura-sync using core error system
//!
//! **CLEANUP**: Replaced custom SyncError enum (389 lines with 11+ variants) with
//! unified AuraError from aura-core. This eliminates redundant error definitions while
//! preserving essential error information through structured messages.
//!
//! Following the pattern established by aura-store, all sync errors now use
//! AuraError with appropriate variant selection and rich error messages.

use aura_core::{AuraError, DeviceId};
use std::time::Duration;

/// Unified result type for all sync operations using core error system
pub type SyncResult<T> = Result<T, AuraError>;

/// Convenience type alias for backward compatibility
pub type SyncError = AuraError;

/// Convenience functions for sync-specific error types.
/// These now map to unified AuraError variants.
///
/// Create a protocol error (maps to Internal).
pub fn sync_protocol_error(protocol: impl Into<String>, message: impl Into<String>) -> AuraError {
    AuraError::internal(format!(
        "Protocol error in {}: {}",
        protocol.into(),
        message.into()
    ))
}

/// Create a protocol error with peer context (maps to Internal).
pub fn sync_protocol_with_peer(
    protocol: impl Into<String>,
    message: impl Into<String>,
    peer: DeviceId,
) -> AuraError {
    AuraError::internal(format!(
        "Protocol error in {} with peer {}: {}",
        protocol.into(),
        peer,
        message.into()
    ))
}

/// Create a sync network error (maps to Network).
pub fn sync_network_error(message: impl Into<String>) -> AuraError {
    AuraError::network(format!("Sync network error: {}", message.into()))
}

/// Create a sync network error with peer context (maps to Network).
pub fn sync_network_with_peer(message: impl Into<String>, peer: DeviceId) -> AuraError {
    AuraError::network(format!(
        "Sync network error with peer {}: {}",
        peer,
        message.into()
    ))
}

/// Create a sync validation error (maps to Invalid).
pub fn sync_validation_error(message: impl Into<String>) -> AuraError {
    AuraError::invalid(format!("Sync validation error: {}", message.into()))
}

/// Create a sync validation error for a specific field (maps to Invalid).
pub fn sync_validation_field_error(
    message: impl Into<String>,
    field: impl Into<String>,
) -> AuraError {
    AuraError::invalid(format!(
        "Sync validation error in field '{}': {}",
        field.into(),
        message.into()
    ))
}

/// Create a sync session error (maps to Internal).
pub fn sync_session_error(message: impl Into<String>) -> AuraError {
    AuraError::internal(format!("Sync session error: {}", message.into()))
}

/// Create a sync session error with session ID (maps to Internal).
pub fn sync_session_with_id(message: impl Into<String>, session_id: uuid::Uuid) -> AuraError {
    AuraError::internal(format!(
        "Sync session error {}: {}",
        session_id,
        message.into()
    ))
}

/// Create a sync configuration error (maps to Invalid).
pub fn sync_config_error(component: impl Into<String>, message: impl Into<String>) -> AuraError {
    AuraError::invalid(format!(
        "Sync configuration error in {}: {}",
        component.into(),
        message.into()
    ))
}

/// Create a sync peer error (maps to Internal).
pub fn sync_peer_error(operation: impl Into<String>, message: impl Into<String>) -> AuraError {
    AuraError::internal(format!(
        "Sync peer error during '{}': {}",
        operation.into(),
        message.into()
    ))
}

/// Create a sync peer error with device ID (maps to Internal).
pub fn sync_peer_with_device(
    operation: impl Into<String>,
    message: impl Into<String>,
    peer: DeviceId,
) -> AuraError {
    AuraError::internal(format!(
        "Sync peer error during '{}' with {}: {}",
        operation.into(),
        peer,
        message.into()
    ))
}

/// Create a sync authorization error (maps to PermissionDenied).
pub fn sync_authorization_error(message: impl Into<String>) -> AuraError {
    AuraError::permission_denied(format!("Sync authorization error: {}", message.into()))
}

/// Create a sync authorization error with capability context (maps to PermissionDenied).
pub fn sync_authorization_capability(
    message: impl Into<String>,
    capability: impl Into<String>,
    peer: DeviceId,
) -> AuraError {
    AuraError::permission_denied(format!(
        "Sync authorization error with peer {}, capability '{}': {}",
        peer,
        capability.into(),
        message.into()
    ))
}

/// Create a sync authorization error from Biscuit token evaluation (maps to PermissionDenied).
pub fn sync_biscuit_authorization_error(message: impl Into<String>, peer: DeviceId) -> AuraError {
    AuraError::permission_denied(format!(
        "Sync Biscuit authorization error with peer {}: {}",
        peer,
        message.into()
    ))
}

/// Create a sync authorization error from Biscuit guard evaluation error.
pub fn sync_biscuit_guard_error(
    guard_capability: impl Into<String>,
    peer: DeviceId,
    error: aura_protocol::guards::GuardError,
) -> AuraError {
    AuraError::permission_denied(format!(
        "Sync Biscuit guard error with peer {}, capability '{}': {}",
        peer,
        guard_capability.into(),
        error
    ))
}

/// Create a sync timeout error (maps to Internal).
pub fn sync_timeout_error(operation: impl Into<String>, duration: Duration) -> AuraError {
    AuraError::internal(format!(
        "Sync operation '{}' timed out after {:?}",
        operation.into(),
        duration
    ))
}

/// Create a sync timeout error with peer context (maps to Internal).
pub fn sync_timeout_with_peer(
    operation: impl Into<String>,
    duration: Duration,
    peer: DeviceId,
) -> AuraError {
    AuraError::internal(format!(
        "Sync operation '{}' with peer {} timed out after {:?}",
        operation.into(),
        peer,
        duration
    ))
}

/// Create a sync resource exhaustion error (maps to Internal).
pub fn sync_resource_exhausted(
    resource: impl Into<String>,
    message: impl Into<String>,
) -> AuraError {
    AuraError::internal(format!(
        "Sync resource '{}' exhausted: {}",
        resource.into(),
        message.into()
    ))
}

/// Create a sync resource exhaustion error with limit (maps to Internal).
pub fn sync_resource_with_limit(
    resource: impl Into<String>,
    message: impl Into<String>,
    limit: u64,
) -> AuraError {
    AuraError::internal(format!(
        "Sync resource '{}' exhausted (limit {}): {}",
        resource.into(),
        limit,
        message.into()
    ))
}

/// Create a sync serialization error (maps to Serialization).
pub fn sync_serialization_error(
    data_type: impl Into<String>,
    message: impl Into<String>,
) -> AuraError {
    AuraError::serialization(format!(
        "Sync serialization error for {}: {}",
        data_type.into(),
        message.into()
    ))
}

/// Create a sync consistency error (maps to Internal).
pub fn sync_consistency_error(
    operation: impl Into<String>,
    message: impl Into<String>,
) -> AuraError {
    AuraError::internal(format!(
        "Sync consistency error during '{}': {}",
        operation.into(),
        message.into()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(test)]
    use aura_core::test_utils::test_device_id;

    #[test]
    fn test_sync_error_creation() {
        let err = sync_protocol_error("anti_entropy", "sync failed");
        assert!(err
            .to_string()
            .contains("Protocol error in anti_entropy: sync failed"));
    }

    #[test]
    fn test_sync_network_error() {
        let err = sync_network_error("connection refused");
        assert!(err
            .to_string()
            .contains("Sync network error: connection refused"));
    }

    #[test]
    fn test_sync_timeout() {
        let err = sync_timeout_error("journal_sync", Duration::from_secs(30));
        assert!(err.to_string().contains("journal_sync"));
        assert!(err.to_string().contains("timed out"));

        let peer = test_device_id(1);
        let err = sync_timeout_with_peer("discovery", Duration::from_secs(30), peer);
        assert!(err.to_string().contains(&peer.to_string()));
    }

    #[test]
    fn test_sync_result_type() {
        fn test_function() -> SyncResult<i32> {
            Ok(42)
        }

        let result = test_function();
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 42);
    }
}
