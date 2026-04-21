use crate::runtime::services::ServiceError;
use aura_app::IntentError;
use std::fmt::Display;

/// Runtime bridge error translation contract for L5 -> L6 -> L7 composition.
///
/// Layer 5 crates may keep crate-local error styles. The runtime bridge is the
/// normalization boundary that classifies failures into stable frontend-visible
/// `IntentError` categories before they cross into `aura-app` and Layer 7.
pub(super) fn bridge_internal(operation: &'static str, error: impl Display) -> IntentError {
    IntentError::internal_error(format!("{operation}: {error}"))
}

pub(super) fn bridge_validation(operation: &'static str, error: impl Display) -> IntentError {
    IntentError::validation_failed(format!("{operation}: {error}"))
}

pub(super) fn bridge_validation_message(reason: impl Into<String>) -> IntentError {
    IntentError::validation_failed(reason)
}

pub(super) fn bridge_network(operation: &'static str, error: impl Display) -> IntentError {
    IntentError::network_error(format!("{operation}: {error}"))
}

pub(super) fn bridge_network_message(reason: impl Into<String>) -> IntentError {
    IntentError::network_error(reason)
}

pub(super) fn bridge_storage(operation: &'static str, error: impl Display) -> IntentError {
    IntentError::storage_error(format!("{operation}: {error}"))
}

pub(super) fn bridge_service(error: ServiceError) -> IntentError {
    IntentError::service_error(error.to_string())
}

pub(super) fn bridge_service_unavailable(service: &'static str) -> IntentError {
    bridge_service(ServiceError::unavailable(service, "service unavailable"))
}

pub(super) fn bridge_service_unavailable_with_detail(
    service: &'static str,
    detail: impl Display,
) -> IntentError {
    bridge_service(ServiceError::unavailable(service, format!("{detail}")))
}

#[cfg(test)]
mod tests {
    use super::{
        bridge_internal, bridge_network, bridge_service_unavailable, bridge_storage,
        bridge_validation,
    };
    use aura_app::IntentError;

    #[test]
    fn bridge_internal_preserves_operation_context() {
        let error = bridge_internal("Persist authority record failed", "disk full");
        assert!(matches!(error, IntentError::InternalError { .. }));
        assert_eq!(
            error.to_string(),
            "Persist authority record failed: disk full"
        );
    }

    #[test]
    fn bridge_error_categories_remain_distinct() {
        let validation = bridge_validation("Invalid peer id", "bad format");
        let storage = bridge_storage("Read account config failed", "permission denied");
        let network = bridge_network("Trigger discovery failed", "timeout");
        let unavailable = bridge_service_unavailable("sync_service");

        assert!(matches!(validation, IntentError::ValidationFailed { .. }));
        assert!(matches!(storage, IntentError::StorageError { .. }));
        assert!(matches!(network, IntentError::NetworkError { .. }));
        assert!(matches!(unavailable, IntentError::ServiceError { .. }));
    }
}
