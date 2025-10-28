//! Protocol wrapper for polymorphic protocol handling
//!
//! This module provides the `ProtocolWrapper` enum that allows storing
//! and handling different protocol types through the unified lifecycle architecture.

use crate::protocols::{RecoveryLifecycle, ResharingLifecycle};
use aura_types::{DeviceId, GuardianId};
use protocol_core::lifecycle::ProtocolLifecycle;

/// Protocol wrapper enum for lifecycle-based protocols
#[derive(Debug, Clone)]
pub enum ProtocolWrapper {
    /// Recovery protocol using lifecycle architecture
    Recovery(RecoveryLifecycle),
    /// Resharing protocol using lifecycle architecture
    Resharing(ResharingLifecycle),
}

impl ProtocolWrapper {
    /// Get the protocol type as a string
    pub fn protocol_type(&self) -> &'static str {
        match self {
            ProtocolWrapper::Recovery(_) => "Recovery",
            ProtocolWrapper::Resharing(_) => "Resharing",
        }
    }

    /// Extract recovery protocol if this is a recovery protocol
    pub fn as_recovery(&self) -> Option<&RecoveryLifecycle> {
        match self {
            ProtocolWrapper::Recovery(recovery) => Some(recovery),
            _ => None,
        }
    }

    /// Extract resharing protocol if this is a resharing protocol
    pub fn as_resharing(&self) -> Option<&ResharingLifecycle> {
        match self {
            ProtocolWrapper::Resharing(resharing) => Some(resharing),
            _ => None,
        }
    }

    /// Check if protocol is finished
    pub fn is_final(&self) -> bool {
        match self {
            ProtocolWrapper::Recovery(recovery) => recovery.is_final(),
            ProtocolWrapper::Resharing(resharing) => resharing.is_final(),
        }
    }
}

/// Error type for protocol wrapper operations
#[derive(Debug, thiserror::Error)]
pub enum ProtocolWrapperError {
    #[error("Wrong protocol type: expected {expected}, got {actual}")]
    WrongProtocolType {
        expected: &'static str,
        actual: &'static str,
    },
    #[error("Protocol error: {0}")]
    ProtocolError(String),
    #[error("Invalid state for operation")]
    InvalidState,
}

/// Builder for creating protocol wrappers using lifecycle architecture
#[derive(Debug, Default)]
pub struct ProtocolWrapperBuilder {
    device_id: Option<DeviceId>,
}

impl ProtocolWrapperBuilder {
    /// Create a new protocol wrapper builder
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the device ID
    pub fn with_device_id(mut self, device_id: DeviceId) -> Self {
        self.device_id = Some(device_id);
        self
    }

    /// Build a recovery protocol using lifecycle architecture
    pub fn build_recovery(
        self,
        approving_guardians: Vec<GuardianId>,
        new_device_id: DeviceId,
    ) -> Result<ProtocolWrapper, ProtocolWrapperError> {
        let device_id = self
            .device_id
            .ok_or_else(|| ProtocolWrapperError::ProtocolError("Device ID required".to_string()))?;

        let recovery =
            RecoveryLifecycle::new_ephemeral(device_id, approving_guardians, new_device_id);
        Ok(ProtocolWrapper::Recovery(recovery))
    }

    /// Build a resharing protocol using lifecycle architecture
    pub fn build_resharing(
        self,
        old_participants: Vec<DeviceId>,
        new_participants: Vec<DeviceId>,
        threshold: u16,
    ) -> Result<ProtocolWrapper, ProtocolWrapperError> {
        let device_id = self
            .device_id
            .ok_or_else(|| ProtocolWrapperError::ProtocolError("Device ID required".to_string()))?;

        let resharing = ResharingLifecycle::new_ephemeral(
            device_id,
            old_participants,
            new_participants,
            threshold,
        );
        Ok(ProtocolWrapper::Resharing(resharing))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_protocol_wrapper_builder() {
        let device_id = DeviceId(Uuid::new_v4());
        let new_device_id = DeviceId(Uuid::new_v4());

        // Build recovery protocol
        let result = ProtocolWrapperBuilder::new()
            .with_device_id(device_id)
            .build_recovery(vec![], new_device_id);

        assert!(result.is_ok());
        let protocol = result.unwrap();
        assert_eq!(protocol.protocol_type(), "Recovery");
    }

    #[test]
    fn test_resharing_protocol_builder() {
        let device_id = DeviceId(Uuid::new_v4());

        // Build resharing protocol
        let result = ProtocolWrapperBuilder::new()
            .with_device_id(device_id)
            .build_resharing(vec![], vec![], 2);

        assert!(result.is_ok());
        let protocol = result.unwrap();
        assert_eq!(protocol.protocol_type(), "Resharing");
    }
}
