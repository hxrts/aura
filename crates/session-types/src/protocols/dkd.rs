//! Session Type States for DKD Choreography (Refactored with Macros)
//!
//! This module defines the session type states for the DKD (Deterministic Key Derivation)
//! protocol, providing compile-time safety for state transitions.

use crate::core::{ChoreographicProtocol, SessionProtocol, SessionState};
use crate::witnesses::{
    CollectedCommitments, CommitmentConfig, RevealConfig, RuntimeWitness, VerifiedReveals,
};
use aura_journal::{DeviceId, Event};
use uuid::Uuid;

// ========== DKD Protocol Core ==========

/// Core data structure for DKD protocol
#[derive(Debug, Clone)]
pub struct DkdProtocolCore {
    /// Device ID for this protocol instance
    device_id: DeviceId,
    /// Protocol session ID
    session_id: Uuid,
    /// App ID for DKD
    app_id: String,
    /// Context label for DKD
    context: String,
}

impl DkdProtocolCore {
    pub fn new(device_id: DeviceId, app_id: String, context: String) -> Self {
        Self {
            device_id,
            session_id: Uuid::new_v4(),
            app_id,
            context,
        }
    }
}

// ========== Error Type ==========

/// Error type for DKD session protocols
#[derive(Debug, thiserror::Error)]
pub enum DkdSessionError {
    #[error("Protocol error: {0}")]
    ProtocolError(String),
    #[error("Invalid state transition")]
    InvalidTransition,
    #[error("Insufficient participants")]
    InsufficientParticipants,
    #[error("Timeout occurred")]
    Timeout,
}

// ========== Protocol Definition using Macros ==========

define_protocol! {
    Protocol: DkdProtocol,
    Core: DkdProtocolCore,
    Error: DkdSessionError,
    Union: DkdProtocolState,

    States {
        InitializationPhase => (),
        CommitmentPhase => (),
        RevealPhase => (),
        FinalizationPhase => (),
        CompletionPhase @ final => Vec<u8>,
        Failure @ final => (),
    }

    Extract {
        session_id: |core| core.session_id,
        device_id: |core| core.device_id,
    }
}

// ========== Protocol Type Alias ==========

/// Session-typed DKD protocol wrapper
pub type DkdProtocol<S> = ChoreographicProtocol<DkdProtocolCore, S>;

// ========== Protocol Methods ==========

impl<S: crate::core::SessionState> ChoreographicProtocol<DkdProtocolCore, S> {
    /// Get reference to the protocol core
    pub fn core(&self) -> &DkdProtocolCore {
        &self.inner
    }

    /// Get the app ID
    pub fn app_id(&self) -> &str {
        &self.core().app_id
    }

    /// Get the context
    pub fn context(&self) -> &str {
        &self.core().context
    }
}

// ========== State Transitions ==========

/// Transition from InitializationPhase to CommitmentPhase (no witness needed)
impl ChoreographicProtocol<DkdProtocolCore, InitializationPhase> {
    /// Begin the commitment phase
    pub fn begin_commitment_phase(self) -> ChoreographicProtocol<DkdProtocolCore, CommitmentPhase> {
        ChoreographicProtocol::transition_to(self)
    }
}

impl ChoreographicProtocol<DkdProtocolCore, CommitmentPhase> {
    /// Transition to reveal phase after collecting sufficient commitments
    pub fn transition_with_collected_commitments(
        self,
        _witness: CollectedCommitments,
    ) -> ChoreographicProtocol<DkdProtocolCore, RevealPhase> {
        ChoreographicProtocol::transition_to(self)
    }
}

/// Transition from RevealPhase to FinalizationPhase (requires VerifiedReveals witness)
impl ChoreographicProtocol<DkdProtocolCore, RevealPhase> {
    /// Transition to FinalizationPhase after verifying reveals
    pub fn transition_with_verified_reveals(
        self,
        _witness: VerifiedReveals,
    ) -> ChoreographicProtocol<DkdProtocolCore, FinalizationPhase> {
        ChoreographicProtocol::transition_to(self)
    }
}

/// Transition from FinalizationPhase to CompletionPhase (no witness needed)
impl ChoreographicProtocol<DkdProtocolCore, FinalizationPhase> {
    /// Complete the protocol successfully
    pub fn complete(self) -> ChoreographicProtocol<DkdProtocolCore, CompletionPhase> {
        ChoreographicProtocol::transition_to(self)
    }
}

/// Transition to Failure state from any state (no witness needed)
impl<S: crate::core::SessionState> ChoreographicProtocol<DkdProtocolCore, S> {
    /// Fail the protocol (can be called from any state)
    pub fn fail(self) -> ChoreographicProtocol<DkdProtocolCore, Failure> {
        ChoreographicProtocol::transition_to(self)
    }
}

// ========== DKD-Specific Operations ==========

/// Operations only available in CommitmentPhase
impl ChoreographicProtocol<DkdProtocolCore, CommitmentPhase> {
    /// Generate and broadcast commitment (only available in CommitmentPhase)
    pub async fn generate_and_broadcast_commitment(&mut self) -> Result<[u8; 32], String> {
        // This would call the underlying protocol's commitment generation
        // For now, return a placeholder
        Ok([0u8; 32])
    }

    /// Check if we can transition to reveal phase
    pub fn can_transition_to_reveal(
        &self,
        events: &[Event],
        threshold: usize,
    ) -> Option<CollectedCommitments> {
        let config = CommitmentConfig {
            threshold,
            session_id: self.protocol_id(),
            expected_participants: None,
        };

        CollectedCommitments::verify(events.to_vec(), config)
    }
}

/// Operations only available in RevealPhase
impl ChoreographicProtocol<DkdProtocolCore, RevealPhase> {
    /// Generate and broadcast reveal (only available in RevealPhase)
    pub async fn generate_and_broadcast_reveal(&mut self) -> Result<Vec<u8>, String> {
        // This would call the underlying protocol's reveal generation
        // For now, return a placeholder
        Ok(vec![0u8; 32])
    }

    /// Check if we can transition to finalization phase
    pub fn can_transition_to_finalization_phase(
        &self,
        events: &[Event],
        commitments: CollectedCommitments,
        threshold: usize,
    ) -> Option<VerifiedReveals> {
        let config = RevealConfig {
            threshold,
            session_id: self.protocol_id(),
        };

        VerifiedReveals::verify((events.to_vec(), commitments), config)
    }
}

/// Operations only available in FinalizationPhase
impl ChoreographicProtocol<DkdProtocolCore, FinalizationPhase> {
    /// Aggregate points and finalize protocol (only available in FinalizationPhase)
    pub async fn aggregate_and_finalize(&mut self) -> Result<Vec<u8>, String> {
        // This would call the underlying protocol's aggregation and finalization
        // For now, return a placeholder derived key
        Ok(vec![1u8; 32])
    }
}

/// Operations available in final states
impl ChoreographicProtocol<DkdProtocolCore, CompletionPhase> {
    /// Get the derived key (only available when complete)
    pub fn get_derived_key(&self) -> Option<Vec<u8>> {
        // This would extract the result from the underlying protocol
        Some(vec![1u8; 32])
    }
}

impl ChoreographicProtocol<DkdProtocolCore, Failure> {
    /// Get the failure reason (only available when failed)
    pub fn get_failure_reason(&self) -> Option<String> {
        // This would extract the error from the underlying protocol
        Some("Protocol failed".to_string())
    }
}

// ========== Factory Functions ==========

/// Create a new DKD protocol in the InitializationPhase state
pub fn new_dkd_protocol(
    device_id: DeviceId,
    app_id: String,
    context: String,
) -> Result<ChoreographicProtocol<DkdProtocolCore, InitializationPhase>, String> {
    let core = DkdProtocolCore::new(device_id, app_id, context);
    Ok(ChoreographicProtocol::new(core))
}

/// Rehydrate a DKD protocol from journal evidence
pub fn rehydrate_dkd_protocol(
    device_id: DeviceId,
    events: &[Event],
    session_id: Uuid,
    app_id: String,
    context: String,
) -> Result<DkdProtocolState, String> {
    // Analyze events to determine current state
    let has_initiate = events
        .iter()
        .any(|e| matches!(e.event_type, aura_journal::EventType::InitiateDkdSession(_)));
    let has_commitments = events.iter().any(|e| {
        matches!(
            e.event_type,
            aura_journal::EventType::RecordDkdCommitment(_)
        )
    });
    let has_reveals = events
        .iter()
        .any(|e| matches!(e.event_type, aura_journal::EventType::RevealDkdPoint(_)));
    let has_finalize = events
        .iter()
        .any(|e| matches!(e.event_type, aura_journal::EventType::FinalizeDkdSession(_)));
    let has_abort = events
        .iter()
        .any(|e| matches!(e.event_type, aura_journal::EventType::AbortDkdSession(_)));

    let mut core = DkdProtocolCore::new(device_id, app_id, context);
    core.session_id = session_id;

    if has_abort {
        Ok(DkdProtocolState::Failure(ChoreographicProtocol::new(core)))
    } else if has_finalize {
        Ok(DkdProtocolState::CompletionPhase(
            ChoreographicProtocol::new(core),
        ))
    } else if has_reveals {
        Ok(DkdProtocolState::FinalizationPhase(
            ChoreographicProtocol::new(core),
        ))
    } else if has_commitments {
        Ok(DkdProtocolState::RevealPhase(ChoreographicProtocol::new(
            core,
        )))
    } else if has_initiate {
        Ok(DkdProtocolState::CommitmentPhase(
            ChoreographicProtocol::new(core),
        ))
    } else {
        Ok(DkdProtocolState::InitializationPhase(
            ChoreographicProtocol::new(core),
        ))
    }
}

// ========== Additional Types for API Compatibility ==========

/// Witness that DKD protocol has completed successfully
#[derive(Debug, Clone)]
pub struct DkdCompleted {
    pub derived_key: Vec<u8>,
    pub session_id: Uuid,
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_crypto::Effects;

    #[test]
    fn test_dkd_state_transitions() {
        let effects = Effects::test();
        let device_id = aura_journal::DeviceId::new_with_effects(&effects);

        let protocol = new_dkd_protocol(
            device_id,
            "test-app".to_string(),
            "test-context".to_string(),
        )
        .unwrap();

        // Should start in InitializationPhase state
        assert_eq!(protocol.state_name(), "InitializationPhase");
        assert!(!protocol.can_terminate());

        // Transition to CommitmentPhase
        let protocol = protocol.begin_commitment_phase();
        assert_eq!(protocol.state_name(), "CommitmentPhase");

        // Can fail from any state
        let failed_protocol = protocol.fail();
        assert_eq!(failed_protocol.state_name(), "Failure");
        assert!(failed_protocol.can_terminate());
    }

    #[test]
    fn test_dkd_state_operations() {
        let effects = Effects::test();
        let device_id = aura_journal::DeviceId::new_with_effects(&effects);

        let protocol = new_dkd_protocol(
            device_id,
            "test-app".to_string(),
            "test-context".to_string(),
        )
        .unwrap();
        let commitment_phase = protocol.begin_commitment_phase();

        // Should be able to call commitment-phase specific operations
        let events = vec![];
        let witness = commitment_phase.can_transition_to_reveal(&events, 2);

        // Should not have sufficient commitments with empty events
        assert!(witness.is_none());
    }

    #[test]
    fn test_dkd_rehydration() {
        let effects = Effects::test();
        let device_id = aura_journal::DeviceId::new_with_effects(&effects);
        let session_id = Uuid::new_v4();

        let events = vec![];

        // Empty events should result in InitializationPhase state
        let state = rehydrate_dkd_protocol(
            device_id,
            &events,
            session_id,
            "test-app".to_string(),
            "test-context".to_string(),
        )
        .unwrap();
        assert_eq!(state.state_name(), "InitializationPhase");
        assert!(!state.can_terminate());
    }
}
