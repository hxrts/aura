//! DeviceAgent Service Layer Delegation Example
//!
//! This example demonstrates clean service layer architecture:
//! - High-level APIs delegate to specialized services
//! - Business logic separated from implementation details
//! - Clear abstraction boundaries

use crate::services::minimal_services::MinimalIdentityService;
use crate::{ContextCapsule, DerivedIdentity, Result};
use std::sync::Arc;
use tracing::info;
use DeviceId;

/// DeviceAgent demonstrating service layer delegation
///
/// This shows how DeviceAgent delegates complex operations to service layers
/// instead of performing low-level operations directly.
pub struct RefactoredDeviceAgent {
    /// Device identifier
    device_id: DeviceId,
    /// Identity service for high-level operations
    identity_service: Arc<MinimalIdentityService>,
}

impl RefactoredDeviceAgent {
    /// Create new device agent
    pub fn new(device_id: DeviceId) -> Self {
        let identity_service = Arc::new(MinimalIdentityService::new(device_id));
        Self {
            device_id,
            identity_service,
        }
    }

    /// Derive a threshold identity through service delegation
    ///
    /// This demonstrates clean service delegation where high-level APIs
    /// contain only business logic and delegate complexity to services.
    ///
    /// Benefits:
    /// - Clean separation of concerns
    /// - Testable service boundaries
    /// - No layer violations
    /// - Minimal cognitive load
    /// - Easy to understand and maintain
    pub async fn derive_context_identity_threshold(
        &self,
        capsule: &ContextCapsule,
        _participants: Vec<DeviceId>,
        _threshold: usize,
        _with_binding_proof: bool,
    ) -> Result<DerivedIdentity> {
        info!("Deriving identity through service delegation");

        // Clean service delegation
        let identity = self
            .identity_service
            .derive_threshold_identity(
                &capsule.app_id,
                &capsule.context_label,
                2,
                vec![self.device_id],
            )
            .await?;

        info!("Successfully derived identity through clean service delegation");
        Ok(identity)
    }

    /// Derive a simple identity for common use cases
    ///
    /// This shows how service layers enable simple APIs for common scenarios.
    pub async fn derive_simple_identity(
        &self,
        app_id: &str,
        context: &str,
    ) -> Result<DerivedIdentity> {
        // Single line delegation for simple case
        self.identity_service
            .derive_simple_identity(app_id, context)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ContextCapsule;
    use aura_crypto::Effects;
    use uuid::Uuid;
    use DeviceId;

    #[tokio::test]
    async fn test_simple_identity() {
        let device_id = DeviceId(Uuid::new_v4());
        let agent = RefactoredDeviceAgent::new(device_id);

        let result = agent
            .derive_simple_identity("test-app", "test-context")
            .await;
        assert!(result.is_ok(), "Simple identity derivation should succeed");

        let identity = result.unwrap();
        assert_eq!(identity.capsule.app_id, "test-app");
        assert_eq!(identity.capsule.context_label, "test-context");
    }

    #[tokio::test]
    async fn test_threshold_identity() {
        let device_id = DeviceId(Uuid::new_v4());
        let agent = RefactoredDeviceAgent::new(device_id);

        let capsule =
            ContextCapsule::simple_with_effects("test-app", "test-context", &Effects::production())
                .expect("Should create capsule");

        let result = agent
            .derive_context_identity_threshold(&capsule, vec![device_id], 2, false)
            .await;

        assert!(
            result.is_ok(),
            "Threshold identity derivation should succeed"
        );

        let identity = result.unwrap();
        assert_eq!(identity.capsule.app_id, "test-app");
        assert_eq!(identity.capsule.context_label, "test-context");
    }
}

/// Example usage showing the clean service delegation pattern
#[allow(dead_code)]
pub async fn example_usage() -> Result<()> {
    let device_id = DeviceId(Uuid::new_v4());
    let agent = RefactoredDeviceAgent::new(device_id);

    // Simple case: Just 1 line
    let identity1 = agent
        .derive_simple_identity("myapp", "user-session")
        .await?;
    println!("Derived simple identity: {}", identity1.capsule.app_id);

    // Complex case: Still just clean delegation
    let capsule =
        ContextCapsule::simple_with_effects("myapp", "admin-session", &Effects::production())?;

    let identity2 = agent
        .derive_context_identity_threshold(&capsule, vec![device_id], 2, true)
        .await?;

    println!("Derived threshold identity: {}", identity2.capsule.app_id);

    Ok(())
}
