//! Protocol wrapper for polymorphic protocol handling
//!
//! This module provides the `ProtocolWrapper` enum that allows storing
//! and handling different protocol types (DKD, Recovery, Resharing)
//! through a common interface.

use crate::protocols::{
    recovery::{new_recovery_protocol, rehydrate_recovery_protocol, RecoveryProtocolState},
    resharing::{new_resharing_protocol, rehydrate_resharing_protocol, ResharingProtocolState},
};
use crate::session_types::wrapper::SessionProtocol;
use aura_journal::Event;
use aura_types::{DeviceId, GuardianId};
use uuid::Uuid;

/// Protocol wrapper enum that can hold any protocol type
///
/// This enum provides type erasure for protocols, allowing them to be
/// stored in collections or passed through APIs that don't use generics.
#[derive(Debug, Clone)]
pub enum ProtocolWrapper {
    /// Recovery protocol
    Recovery(RecoveryProtocolState),
    /// Resharing protocol
    Resharing(ResharingProtocolState),
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
    pub fn as_recovery(&self) -> Option<&RecoveryProtocolState> {
        match self {
            ProtocolWrapper::Recovery(recovery) => Some(recovery),
            _ => None,
        }
    }

    /// Extract resharing protocol if this is a resharing protocol
    pub fn as_resharing(&self) -> Option<&ResharingProtocolState> {
        match self {
            ProtocolWrapper::Resharing(resharing) => Some(resharing),
            _ => None,
        }
    }
}

impl SessionProtocol for ProtocolWrapper {
    fn session_id(&self) -> Uuid {
        match self {
            ProtocolWrapper::Recovery(recovery) => recovery.session_id(),
            ProtocolWrapper::Resharing(resharing) => resharing.session_id(),
        }
    }

    fn state_name(&self) -> &'static str {
        match self {
            ProtocolWrapper::Recovery(recovery) => recovery.state_name(),
            ProtocolWrapper::Resharing(resharing) => resharing.state_name(),
        }
    }

    fn is_final(&self) -> bool {
        match self {
            ProtocolWrapper::Recovery(recovery) => recovery.is_final(),
            ProtocolWrapper::Resharing(resharing) => resharing.is_final(),
        }
    }

    fn can_terminate(&self) -> bool {
        match self {
            ProtocolWrapper::Recovery(recovery) => recovery.can_terminate(),
            ProtocolWrapper::Resharing(resharing) => resharing.can_terminate(),
        }
    }

    fn protocol_id(&self) -> Uuid {
        match self {
            ProtocolWrapper::Recovery(recovery) => recovery.protocol_id(),
            ProtocolWrapper::Resharing(resharing) => resharing.protocol_id(),
        }
    }

    fn device_id(&self) -> Uuid {
        match self {
            ProtocolWrapper::Recovery(recovery) => recovery.device_id(),
            ProtocolWrapper::Resharing(resharing) => resharing.device_id().0,
        }
    }
}

/// Trait for converting protocol states into protocol wrapper
pub trait IntoProtocolWrapper {
    /// Convert this protocol state into a protocol wrapper
    fn into_wrapper(self) -> ProtocolWrapper;
}


impl IntoProtocolWrapper for RecoveryProtocolState {
    fn into_wrapper(self) -> ProtocolWrapper {
        ProtocolWrapper::Recovery(self)
    }
}

impl IntoProtocolWrapper for ResharingProtocolState {
    fn into_wrapper(self) -> ProtocolWrapper {
        ProtocolWrapper::Resharing(self)
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

/// Builder for creating protocol wrappers
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


    /// Build a recovery protocol
    pub fn build_recovery(
        self,
        recovery_id: Uuid,
        guardian_set: Vec<GuardianId>,
        threshold: u16,
    ) -> Result<ProtocolWrapper, ProtocolWrapperError> {
        let device_id = self
            .device_id
            .ok_or_else(|| ProtocolWrapperError::ProtocolError("Device ID required".to_string()))?;

        new_recovery_protocol(recovery_id, device_id, guardian_set, threshold, None)
            .map(|recovery| ProtocolWrapper::Recovery(recovery))
            .map_err(|e| ProtocolWrapperError::ProtocolError(e.to_string()))
    }

    /// Build a resharing protocol
    pub fn build_resharing(
        self,
        session_id: Uuid,
        old_threshold: u16,
        new_threshold: u16,
        old_participants: Vec<DeviceId>,
        new_participants: Vec<DeviceId>,
    ) -> Result<ProtocolWrapper, ProtocolWrapperError> {
        let device_id = self
            .device_id
            .ok_or_else(|| ProtocolWrapperError::ProtocolError("Device ID required".to_string()))?;

        new_resharing_protocol(
            session_id,
            device_id,
            new_threshold,
            old_threshold,
            new_participants,
            old_participants,
        )
        .map(|resharing| ProtocolWrapper::Resharing(resharing))
        .map_err(|e| ProtocolWrapperError::ProtocolError(e.to_string()))
    }
}

/// Helper function to rehydrate a protocol from journal evidence
pub fn rehydrate_protocol(
    protocol_type: &str,
    device_id: DeviceId,
    evidence: Vec<Event>,
    context: Option<String>,
) -> Result<ProtocolWrapper, ProtocolWrapperError> {
    match protocol_type {
        "Recovery" => {
            // Extract recovery parameters from evidence
            let recovery_id = Uuid::new_v4(); // TODO: Extract from evidence
            let guardian_set = vec![]; // TODO: Extract from evidence
            let threshold = 2; // TODO: Extract from evidence

            rehydrate_recovery_protocol(
                recovery_id,
                device_id,
                guardian_set,
                threshold,
                None,
                evidence,
            )
            .map(|recovery| ProtocolWrapper::Recovery(recovery))
            .map_err(|e| ProtocolWrapperError::ProtocolError(e.to_string()))
        }
        "Resharing" => {
            // Extract resharing parameters from evidence
            let old_threshold = 2; // TODO: Extract from evidence
            let new_threshold = 3; // TODO: Extract from evidence
            let old_participants = vec![]; // TODO: Extract from evidence
            let new_participants = vec![]; // TODO: Extract from evidence

            // Extract session_id from evidence or use new one
            let session_id = Uuid::new_v4(); // TODO: Extract from evidence
            rehydrate_resharing_protocol(
                session_id,
                device_id,
                new_threshold,
                old_threshold,
                new_participants,
                old_participants,
                evidence,
            )
            .map(|resharing| ProtocolWrapper::Resharing(resharing))
            .map_err(|e| ProtocolWrapperError::ProtocolError(e.to_string()))
        }
        _ => Err(ProtocolWrapperError::ProtocolError(format!(
            "Unknown protocol type: {}",
            protocol_type
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_protocol_wrapper_builder() {
        let device_id = DeviceId(Uuid::new_v4());

        // Build DKD protocol
        let result = ProtocolWrapperBuilder::new()
            .with_device_id(device_id)
            .build_dkd("test-app".to_string(), "test-context".to_string());

        assert!(result.is_ok());
        let protocol = result.unwrap();
        assert_eq!(protocol.protocol_type(), "DKD");
    }

    #[test]
    fn test_protocol_conversion() {
        let device_id = DeviceId(Uuid::new_v4());
        let dkd = new_dkd_protocol(device_id, "app".to_string(), "context".to_string()).unwrap();

        let wrapper = dkd.into_wrapper();
        assert_eq!(wrapper.protocol_type(), "DKD");
        assert!(!wrapper.is_final());
    }
}
