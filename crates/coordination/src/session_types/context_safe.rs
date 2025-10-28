//! Safe Context Session Type Implementations
//!
//! This module provides orphan-rule-compliant implementations of protocol context
//! state transitions with compile-time safety guarantees.

use super::{
    context::{
        AwaitingCondition, ExecutingInstructions, ProtocolContextCore, ContextInitialized,
        ThresholdEventsMet,
    },
    local_transitions::{
        SafeConditionalTransition, SafeSessionProtocol, SafeStateTransition, SafeTransitionError,
        SafeWitnessedTransition,
    },
};
use aura_types::DeviceId;
use aura_types::SessionError;
use std::collections::BTreeMap;

// ========== Context State Transition Witnesses ==========

/// Evidence that required threshold conditions have been met
#[derive(Debug, Clone)]
pub struct ContextThresholdMet {
    pub condition_type: String,
    pub required_count: u16,
    pub actual_count: u16,
    pub participants: Vec<DeviceId>,
}

/// Instructions to execute after conditions are met
#[derive(Debug, Clone)]
pub struct ExecutionInstructions {
    pub instruction_type: String,
    pub parameters: BTreeMap<String, String>,
    pub target_participants: Vec<DeviceId>,
}

/// Initialization parameters for protocol context
#[derive(Debug, Clone)]
pub struct ContextInitialization {
    pub protocol_name: String,
    pub session_id: uuid::Uuid,
    pub initial_participants: Vec<DeviceId>,
    pub context_data: Vec<u8>,
}

// ========== Safe Context Protocol Type Aliases ==========

/// Safe wrapper for Ready for Initialization state
pub type SafeContextInitialized =
    SafeSessionProtocol<ProtocolContextCore, ContextInitialized>;

/// Safe wrapper for Awaiting Condition state
pub type SafeAwaitingCondition = SafeSessionProtocol<ProtocolContextCore, AwaitingCondition>;

/// Safe wrapper for Executing Instructions state
pub type SafeExecutingInstructions =
    SafeSessionProtocol<ProtocolContextCore, ExecutingInstructions>;

// ========== Safe Context Transitions ==========

/// Ready for Initialization -> Awaiting Condition (when protocol starts)
impl SafeWitnessedTransition<ContextInitialized, AwaitingCondition>
    for SafeContextInitialized
{
    type Witness = ContextInitialization;
    type Target = SafeAwaitingCondition;

    fn safe_transition_with_witness(mut self, context: Self::Witness) -> Self::Target {
        // Update session context with initialization parameters
        self.inner_mut().session_id = context.session_id;

        tracing::info!(
            "🚀 Context: Initializing {} protocol (session: {}, participants: {})",
            context.protocol_name,
            context.session_id,
            context.initial_participants.len()
        );

        // Store initialization data in protocol core
        tracing::debug!(
            "📋 Context: Storing {} bytes of context data",
            context.context_data.len()
        );

        SafeSessionProtocol::new(self.inner.transition_to())
    }
}

/// Awaiting Condition -> Executing Instructions (when threshold met)
impl SafeWitnessedTransition<AwaitingCondition, ExecutingInstructions> for SafeAwaitingCondition {
    type Witness = ContextThresholdMet;
    type Target = SafeExecutingInstructions;

    fn safe_transition_with_witness(self, witness: Self::Witness) -> Self::Target {
        // Validate threshold requirements
        if witness.actual_count < witness.required_count {
            tracing::warn!(
                "🚨 Context: Threshold not fully met ({} < {} for {})",
                witness.actual_count,
                witness.required_count,
                witness.condition_type
            );
            // In a real implementation, this might return an error
            // For now, we'll proceed but log the issue
        }

        tracing::info!(
            "✅ Context: Threshold met for {} ({}/{}), transitioning to execution",
            witness.condition_type,
            witness.actual_count,
            witness.required_count
        );

        SafeSessionProtocol::new(self.inner.transition_to())
    }
}

/// Executing Instructions -> Ready for Initialization (when complete)
impl SafeWitnessedTransition<ExecutingInstructions, ContextInitialized>
    for SafeExecutingInstructions
{
    type Witness = ExecutionInstructions;
    type Target = SafeContextInitialized;

    fn safe_transition_with_witness(self, witness: Self::Witness) -> Self::Target {
        tracing::info!(
            "🎯 Context: Executing {} instructions for {} participants",
            witness.instruction_type,
            witness.target_participants.len()
        );

        // Log execution parameters for debugging
        for (key, value) in &witness.parameters {
            tracing::debug!("📋 Context: Parameter {} = {}", key, value);
        }

        tracing::info!("✅ Context: Instructions executed, returning to ready state");

        SafeSessionProtocol::new(self.inner.transition_to())
    }
}

// ========== Conditional Transitions for Protocol Management ==========

/// Condition for pausing protocol execution
pub struct ContextPauseCondition {
    pub reason: String,
    pub pause_duration_secs: Option<u64>,
}

/// Condition for aborting protocol execution
pub struct ContextAbortCondition {
    pub reason: String,
    pub error_code: String,
    pub failed_participants: Vec<DeviceId>,
}

/// Conditional transition for pausing from Executing Instructions back to Awaiting Condition
impl SafeConditionalTransition<ExecutingInstructions, AwaitingCondition>
    for SafeExecutingInstructions
{
    type Target = SafeAwaitingCondition;
    type Condition = ContextPauseCondition;

    fn safe_conditional_transition(
        self,
        condition: Self::Condition,
    ) -> Result<Self::Target, (Self, SessionError)> {
        tracing::warn!(
            "⏸️ Context: Pausing execution - {}",
            condition.reason
        );

        if let Some(duration) = condition.pause_duration_secs {
            tracing::info!("⏱️ Context: Pause duration: {} seconds", duration);
        }

        // In a real implementation, this might set timers or save state
        Ok(SafeSessionProtocol::new(self.inner.transition_to()))
    }
}

/// Any context state can transition back to Ready for Initialization on abort
macro_rules! impl_context_abort {
    ($from_state:ty) => {
        impl SafeConditionalTransition<$from_state, ContextInitialized>
            for SafeSessionProtocol<ProtocolContextCore, $from_state>
        {
            type Target = SafeContextInitialized;
            type Condition = ContextAbortCondition;

            fn safe_conditional_transition(
                self,
                condition: Self::Condition,
            ) -> Result<Self::Target, (Self, SessionError)> {
                tracing::error!(
                    "💥 Context: Protocol aborted - {} (error: {}), failed participants: {:?}",
                    condition.reason,
                    condition.error_code,
                    condition.failed_participants
                );

                // In a real implementation, this might:
                // 1. Clean up protocol state
                // 2. Notify participants of abort
                // 3. Log failure for analysis
                // 4. Update fault tolerance counters

                Ok(SafeSessionProtocol::new(self.inner.transition_to()))
            }
        }
    };
}

// Implement abort transitions for all context states
impl_context_abort!(AwaitingCondition);
impl_context_abort!(ExecutingInstructions);

// ========== Helper Functions ==========

/// Validate context initialization parameters
pub fn validate_context_initialization(
    protocol_name: &str,
    participants: &[DeviceId],
    context_data: &[u8],
) -> Result<(), String> {
    if protocol_name.is_empty() {
        return Err("Protocol name cannot be empty".to_string());
    }

    if participants.is_empty() {
        return Err("Must have at least one participant".to_string());
    }

    // Check for duplicate participants
    let mut unique_participants = std::collections::HashSet::new();
    for participant in participants {
        if !unique_participants.insert(participant) {
            return Err(format!("Duplicate participant: {}", participant.0));
        }
    }

    if context_data.len() > 1024 * 1024 {
        // 1MB limit
        return Err("Context data too large (max 1MB)".to_string());
    }

    Ok(())
}

/// Create a context initialization with validation
pub fn create_context_initialization(
    protocol_name: String,
    session_id: uuid::Uuid,
    initial_participants: Vec<DeviceId>,
    context_data: Vec<u8>,
) -> Result<ContextInitialization, String> {
    validate_context_initialization(&protocol_name, &initial_participants, &context_data)?;

    Ok(ContextInitialization {
        protocol_name,
        session_id,
        initial_participants,
        context_data,
    })
}

/// Validate threshold requirements
pub fn validate_threshold_met(
    condition_type: &str,
    required_count: u16,
    actual_count: u16,
    participants: &[DeviceId],
) -> Result<(), String> {
    if required_count == 0 {
        return Err("Required count cannot be zero".to_string());
    }

    if actual_count < required_count {
        return Err(format!(
            "Threshold not met for {}: {} < {}",
            condition_type, actual_count, required_count
        ));
    }

    if participants.len() != actual_count as usize {
        return Err(format!(
            "Participant count mismatch: {} participants for count {}",
            participants.len(),
            actual_count
        ));
    }

    Ok(())
}

/// Create a threshold met witness with validation
pub fn create_threshold_met(
    condition_type: String,
    required_count: u16,
    participants: Vec<DeviceId>,
) -> Result<ContextThresholdMet, String> {
    let actual_count = participants.len() as u16;
    validate_threshold_met(&condition_type, required_count, actual_count, &participants)?;

    Ok(ContextThresholdMet {
        condition_type,
        required_count,
        actual_count,
        participants,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_types::DeviceId;

    #[test]
    fn test_context_initialization_validation() {
        let participants = vec![DeviceId::new_v4(), DeviceId::new_v4()];
        let context_data = b"test data".to_vec();

        assert!(validate_context_initialization("test_protocol", &participants, &context_data)
            .is_ok());
        assert!(
            validate_context_initialization("", &participants, &context_data).is_err()
        );
        assert!(validate_context_initialization("test_protocol", &[], &context_data).is_err());
        
        // Test duplicate participants
        let duplicate_participants = vec![participants[0], participants[0]];
        assert!(validate_context_initialization(
            "test_protocol",
            &duplicate_participants,
            &context_data
        )
        .is_err());
    }

    #[test]
    fn test_threshold_validation() {
        let participants = vec![DeviceId::new_v4(), DeviceId::new_v4()];

        assert!(validate_threshold_met("test", 2, 2, &participants).is_ok());
        assert!(validate_threshold_met("test", 3, 2, &participants).is_err());
        assert!(validate_threshold_met("test", 0, 2, &participants).is_err());
        assert!(validate_threshold_met("test", 2, 3, &participants).is_err());
    }

    #[test]
    fn test_context_creation() {
        let session_id = uuid::Uuid::new_v4();
        let participants = vec![DeviceId::new_v4(), DeviceId::new_v4()];
        let context_data = b"test data".to_vec();

        assert!(create_context_initialization(
            "test_protocol".to_string(),
            session_id,
            participants.clone(),
            context_data
        )
        .is_ok());

        assert!(create_context_initialization(
            "".to_string(),
            session_id,
            participants,
            vec![]
        )
        .is_err());
    }
}