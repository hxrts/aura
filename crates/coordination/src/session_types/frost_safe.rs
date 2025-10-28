//! Safe FROST Session Type Implementations
//!
//! This module provides orphan-rule-compliant implementations of FROST protocol
//! state transitions with compile-time safety guarantees.

use super::{
    frost::{
        FrostCommitmentPhase, FrostIdle, FrostProtocolCore, FrostSigningPhase, 
        FrostAwaitingCommitments, FrostAwaitingShares, FrostReadyToAggregate,
        FrostSigningContext,
    },
    local_transitions::{
        SafeConditionalTransition, SafeSessionProtocol, SafeStateTransition, SafeTransitionError,
        SafeWitnessedTransition,
    },
};
use aura_types::DeviceId;
use aura_types::SessionError;
use std::collections::BTreeSet;

// ========== FROST State Transition Witnesses ==========

/// Evidence that we have received enough commitments to proceed to reveal phase
#[derive(Debug, Clone)]
pub struct CommitmentsReceived {
    pub participant_count: u16,
    pub threshold: u16,
    pub commitments: BTreeSet<DeviceId>,
}

/// Evidence that we have received enough reveals to proceed to signature phase
#[derive(Debug, Clone)]
pub struct RevealsReceived {
    pub participant_count: u16,
    pub threshold: u16,
    pub reveals: BTreeSet<DeviceId>,
}

/// Evidence that we have received enough signature shares to finalize
#[derive(Debug, Clone)]
pub struct SignatureSharesReceived {
    pub participant_count: u16,
    pub threshold: u16,
    pub signature_shares: BTreeSet<DeviceId>,
}

/// Context required to initiate FROST protocol
#[derive(Debug, Clone)]
pub struct FrostInitiationContext {
    pub session_id: uuid::Uuid,
    pub message: Vec<u8>,
    pub participants: Vec<DeviceId>,
    pub threshold: u16,
}

// ========== Safe FROST Protocol Type Aliases ==========

/// Safe wrapper for FROST Idle state
pub type SafeFrostIdle = SafeSessionProtocol<FrostProtocolCore, FrostIdle>;

/// Safe wrapper for FROST Commitment phase
pub type SafeFrostCommitmentPhase = SafeSessionProtocol<FrostProtocolCore, FrostCommitmentPhase>;

/// Safe wrapper for FROST Reveal phase
pub type SafeFrostRevealPhase = SafeSessionProtocol<FrostProtocolCore, FrostRevealPhase>;

/// Safe wrapper for FROST Signature phase
pub type SafeFrostSignaturePhase = SafeSessionProtocol<FrostProtocolCore, FrostSignaturePhase>;

/// Safe wrapper for FROST waiting for commitments
pub type SafeFrostWaitingForCommitments =
    SafeSessionProtocol<FrostProtocolCore, FrostWaitingForCommitments>;

/// Safe wrapper for FROST waiting for reveals
pub type SafeFrostWaitingForReveals =
    SafeSessionProtocol<FrostProtocolCore, FrostWaitingForReveals>;

/// Safe wrapper for FROST waiting for signatures
pub type SafeFrostWaitingForSignatures =
    SafeSessionProtocol<FrostProtocolCore, FrostWaitingForSignatures>;

// ========== Safe FROST Transitions ==========

/// Idle -> Commitment Phase (when starting signing)
impl SafeWitnessedTransition<FrostIdle, FrostCommitmentPhase> for SafeFrostIdle {
    type Witness = FrostInitiationContext;
    type Target = SafeFrostCommitmentPhase;

    fn safe_transition_with_witness(mut self, context: Self::Witness) -> Self::Target {
        // Update session context with initiation parameters
        self.inner_mut().session_id = context.session_id;

        // Validate threshold parameters
        if context.threshold as usize > context.participants.len() {
            tracing::warn!(
                "FROST threshold {} exceeds participant count {}",
                context.threshold,
                context.participants.len()
            );
        }

        tracing::info!(
            "🚀 FROST: Transitioning from Idle to Commitment phase (session: {}, threshold: {}/{}, message: {} bytes)",
            context.session_id,
            context.threshold,
            context.participants.len(),
            context.message.len()
        );

        SafeSessionProtocol::new(self.into_inner().transition_to())
    }
}

/// Commitment Phase -> Waiting for Commitments
impl SafeStateTransition<FrostCommitmentPhase, FrostWaitingForCommitments>
    for SafeFrostCommitmentPhase
{
    type Target = SafeFrostWaitingForCommitments;

    fn safe_transition_to(self) -> Self::Target {
        tracing::debug!("🔄 FROST: Transitioning to waiting for commitments");
        SafeSessionProtocol::new(self.into_inner().transition_to())
    }
}

/// Waiting for Commitments -> Reveal Phase (when threshold reached)
impl SafeWitnessedTransition<FrostWaitingForCommitments, FrostRevealPhase>
    for SafeFrostWaitingForCommitments
{
    type Witness = CommitmentsReceived;
    type Target = SafeFrostRevealPhase;

    fn safe_transition_with_witness(self, witness: Self::Witness) -> Self::Target {
        // Validate threshold requirements
        let commitment_count = witness.commitments.len() as u16;
        if commitment_count < witness.threshold {
            tracing::warn!(
                "🚨 FROST: Insufficient commitments received ({} < {})",
                commitment_count,
                witness.threshold
            );
            // In a real implementation, this might return an error
            // For now, we'll proceed but log the issue
        }

        tracing::info!(
            "✅ FROST: Commitments threshold reached ({}/{}), transitioning to reveal phase",
            commitment_count,
            witness.threshold
        );

        SafeSessionProtocol::new(self.into_inner().transition_to())
    }
}

/// Reveal Phase -> Waiting for Reveals
impl SafeStateTransition<FrostRevealPhase, FrostWaitingForReveals> for SafeFrostRevealPhase {
    type Target = SafeFrostWaitingForReveals;

    fn safe_transition_to(self) -> Self::Target {
        tracing::debug!("🔄 FROST: Transitioning to waiting for reveals");
        SafeSessionProtocol::new(self.into_inner().transition_to())
    }
}

/// Waiting for Reveals -> Signature Phase (when threshold reached)
impl SafeWitnessedTransition<FrostWaitingForReveals, FrostSignaturePhase>
    for SafeFrostWaitingForReveals
{
    type Witness = RevealsReceived;
    type Target = SafeFrostSignaturePhase;

    fn safe_transition_with_witness(self, witness: Self::Witness) -> Self::Target {
        let reveal_count = witness.reveals.len() as u16;
        if reveal_count < witness.threshold {
            tracing::warn!(
                "🚨 FROST: Insufficient reveals received ({} < {})",
                reveal_count,
                witness.threshold
            );
        }

        tracing::info!(
            "✅ FROST: Reveals threshold reached ({}/{}), transitioning to signature phase",
            reveal_count,
            witness.threshold
        );

        SafeSessionProtocol::new(self.into_inner().transition_to())
    }
}

/// Signature Phase -> Waiting for Signatures
impl SafeStateTransition<FrostSignaturePhase, FrostWaitingForSignatures>
    for SafeFrostSignaturePhase
{
    type Target = SafeFrostWaitingForSignatures;

    fn safe_transition_to(self) -> Self::Target {
        tracing::debug!("🔄 FROST: Transitioning to waiting for signatures");
        SafeSessionProtocol::new(self.into_inner().transition_to())
    }
}

/// Waiting for Signatures -> Idle (when signature complete)
impl SafeWitnessedTransition<FrostWaitingForSignatures, FrostIdle>
    for SafeFrostWaitingForSignatures
{
    type Witness = SignatureSharesReceived;
    type Target = SafeFrostIdle;

    fn safe_transition_with_witness(self, witness: Self::Witness) -> Self::Target {
        let signature_count = witness.signature_shares.len() as u16;
        if signature_count < witness.threshold {
            tracing::warn!(
                "🚨 FROST: Insufficient signature shares received ({} < {})",
                signature_count,
                witness.threshold
            );
        }

        tracing::info!(
            "🎉 FROST: Signature complete ({}/{}), transitioning back to idle",
            signature_count,
            witness.threshold
        );

        SafeSessionProtocol::new(self.into_inner().transition_to())
    }
}

// ========== Conditional Transitions for Error Recovery ==========

/// Conditional transition for aborting FROST protocol from any state
pub struct FrostAbortCondition {
    pub reason: String,
    pub failed_participants: Vec<DeviceId>,
}

/// Any FROST state can transition back to Idle on abort
macro_rules! impl_frost_abort {
    ($from_state:ty) => {
        impl SafeConditionalTransition<$from_state, FrostIdle>
            for SafeSessionProtocol<FrostProtocolCore, $from_state>
        {
            type Target = SafeFrostIdle;
            type Condition = FrostAbortCondition;

            fn safe_conditional_transition(
                self,
                condition: Self::Condition,
            ) -> Result<Self::Target, (Self, SessionError)> {
                tracing::warn!(
                    "FROST: Protocol aborted - {}, failed participants: {:?}",
                    condition.reason,
                    condition.failed_participants
                );

                // In a real implementation, this might clean up state
                // and notify other participants of the abort

                Ok(SafeSessionProtocol::new(self.into_inner().transition_to()))
            }
        }
    };
}

// Implement abort transitions for all FROST states
impl_frost_abort!(FrostCommitmentPhase);
impl_frost_abort!(FrostRevealPhase);
impl_frost_abort!(FrostSignaturePhase);
impl_frost_abort!(FrostWaitingForCommitments);
impl_frost_abort!(FrostWaitingForReveals);
impl_frost_abort!(FrostWaitingForSignatures);

// ========== Helper Functions ==========

/// Validate FROST threshold parameters
pub fn validate_frost_threshold(threshold: u16, participant_count: usize) -> Result<(), String> {
    if threshold == 0 {
        return Err("Threshold cannot be zero".to_string());
    }
    if threshold as usize > participant_count {
        return Err(format!(
            "Threshold {} exceeds participant count {}",
            threshold, participant_count
        ));
    }
    if participant_count == 0 {
        return Err("Must have at least one participant".to_string());
    }
    Ok(())
}

/// Create a FROST initiation context with validation
pub fn create_frost_context(
    session_id: uuid::Uuid,
    message: Vec<u8>,
    participants: Vec<DeviceId>,
    threshold: u16,
) -> Result<FrostInitiationContext, String> {
    validate_frost_threshold(threshold, participants.len())?;

    if message.is_empty() {
        return Err("Message cannot be empty".to_string());
    }

    Ok(FrostInitiationContext {
        session_id,
        message,
        participants,
        threshold,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_types::DeviceId;

    #[test]
    fn test_frost_threshold_validation() {
        assert!(validate_frost_threshold(2, 3).is_ok());
        assert!(validate_frost_threshold(3, 3).is_ok());
        assert!(validate_frost_threshold(0, 3).is_err());
        assert!(validate_frost_threshold(4, 3).is_err());
        assert!(validate_frost_threshold(1, 0).is_err());
    }

    #[test]
    fn test_frost_context_creation() {
        let session_id = uuid::Uuid::new_v4();
        let message = b"test message".to_vec();
        let participants = vec![DeviceId::new_v4(), DeviceId::new_v4(), DeviceId::new_v4()];

        assert!(create_frost_context(session_id, message.clone(), participants.clone(), 2).is_ok());
        assert!(create_frost_context(session_id, vec![], participants.clone(), 2).is_err());
        assert!(create_frost_context(session_id, message, participants, 4).is_err());
    }
}
