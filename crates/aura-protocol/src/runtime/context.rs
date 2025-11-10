//! Execution context for protocol operations
//!
//! Provides the execution environment in which protocols run, including device identity,
//! session information, and access to effect handlers.

use crate::{
    effects::ProtocolEffects,
    handlers::CompositeHandler,
    middleware::{create_standard_stack, MiddlewareConfig},
};
use aura_core::DeviceId;
use uuid::Uuid;

/// Execution context for protocol operations
pub struct ExecutionContext {
    /// Device ID for this execution context
    pub device_id: DeviceId,
    /// Session ID for the current protocol execution
    pub session_id: Uuid,
    /// Participants in the current session
    pub participants: Vec<DeviceId>,
    /// Effect handlers with middleware applied
    pub effects: Box<dyn ProtocolEffects>,
    /// Whether this is a simulation/test environment
    pub is_simulation: bool,
    /// Optional threshold for multi-party protocols
    pub threshold: Option<usize>,
}

impl ExecutionContext {
    /// Create a new execution context
    pub fn new(
        device_id: DeviceId,
        session_id: Uuid,
        participants: Vec<DeviceId>,
        effects: Box<dyn ProtocolEffects>,
        is_simulation: bool,
        threshold: Option<usize>,
    ) -> Self {
        Self {
            device_id,
            session_id,
            participants,
            effects,
            is_simulation,
            threshold,
        }
    }

    /// Get the number of participants in the session
    pub fn participant_count(&self) -> usize {
        self.participants.len()
    }

    /// Check if this device is a participant in the session
    pub fn is_participant(&self, device_id: DeviceId) -> bool {
        self.participants.contains(&device_id)
    }

    /// Get the index of this device in the participant list
    pub fn participant_index(&self) -> Option<usize> {
        self.participants
            .iter()
            .position(|&id| id == self.device_id)
    }

    /// Get the threshold for multi-party protocols
    pub fn threshold(&self) -> Option<usize> {
        self.threshold
    }

    /// Check if we have sufficient participants for the threshold
    pub fn has_sufficient_participants(&self) -> bool {
        if let Some(threshold) = self.threshold {
            self.participant_count() >= threshold
        } else {
            true
        }
    }
}

/// Builder for creating execution contexts
pub struct ContextBuilder {
    device_id: Option<DeviceId>,
    session_id: Option<Uuid>,
    participants: Vec<DeviceId>,
    is_simulation: bool,
    threshold: Option<usize>,
    middleware_config: Option<MiddlewareConfig>,
}

impl ContextBuilder {
    /// Create a new context builder
    pub fn new() -> Self {
        Self {
            device_id: None,
            session_id: None,
            participants: Vec::new(),
            is_simulation: false,
            threshold: None,
            middleware_config: None,
        }
    }

    /// Set the device ID
    pub fn with_device_id(mut self, device_id: DeviceId) -> Self {
        self.device_id = Some(device_id);
        self
    }

    /// Set the session ID
    pub fn with_session_id(mut self, session_id: Uuid) -> Self {
        self.session_id = Some(session_id);
        self
    }

    /// Set the participants
    pub fn with_participants(mut self, participants: Vec<DeviceId>) -> Self {
        self.participants = participants;
        self
    }

    /// Add a participant
    pub fn add_participant(mut self, participant: DeviceId) -> Self {
        self.participants.push(participant);
        self
    }

    /// Enable simulation mode
    pub fn simulation(mut self) -> Self {
        self.is_simulation = true;
        self
    }

    /// Set the threshold for multi-party protocols
    pub fn with_threshold(mut self, threshold: usize) -> Self {
        self.threshold = Some(threshold);
        self
    }

    /// Set middleware configuration
    pub fn with_middleware_config(mut self, config: MiddlewareConfig) -> Self {
        self.middleware_config = Some(config);
        self
    }

    /// Build the execution context for testing
    pub fn build_for_testing(self) -> ExecutionContext {
        let device_id = self.device_id.expect("Device ID is required");
        let session_id = self.session_id.unwrap_or_else(Uuid::new_v4);

        let base_handler = CompositeHandler::for_testing(device_id.into());
        let effects = if let Some(config) = self.middleware_config {
            create_standard_stack(base_handler, config)
        } else {
            base_handler
        };

        ExecutionContext::new(
            device_id,
            session_id,
            self.participants,
            Box::new(effects),
            true,
            self.threshold,
        )
    }

    /// Build the execution context for production
    pub fn build_for_production(self) -> ExecutionContext {
        let device_id = self.device_id.expect("Device ID is required");
        let session_id = self.session_id.unwrap_or_else(Uuid::new_v4);

        let base_handler = CompositeHandler::for_production(device_id.into());
        let effects = if let Some(config) = self.middleware_config {
            create_standard_stack(base_handler, config)
        } else {
            base_handler
        };

        ExecutionContext::new(
            device_id,
            session_id,
            self.participants,
            Box::new(effects),
            false,
            self.threshold,
        )
    }

    /// Build the execution context for simulation
    pub fn build_for_simulation(self) -> ExecutionContext {
        let device_id = self.device_id.expect("Device ID is required");
        let session_id = self.session_id.unwrap_or_else(Uuid::new_v4);

        let base_handler = CompositeHandler::for_simulation(device_id.into());
        let effects = if let Some(config) = self.middleware_config {
            create_standard_stack(base_handler, config)
        } else {
            base_handler
        };

        ExecutionContext::new(
            device_id,
            session_id,
            self.participants,
            Box::new(effects),
            true,
            self.threshold,
        )
    }
}

impl Default for ContextBuilder {
    fn default() -> Self {
        Self::new()
    }
}
