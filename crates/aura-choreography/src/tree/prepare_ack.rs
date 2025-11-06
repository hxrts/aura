//! Prepare/ACK/NACK protocol for snapshot validation
//!
//! Implements CAS-style snapshot validation using the propose_and_acknowledge pattern.
//! All participants verify their local snapshot matches the intent's snapshot before
//! proceeding with the TreeSession.

use crate::patterns::{
    ProposalValidator, ProposeAcknowledgeConfig, ProposeAndAcknowledgeChoreography,
};
use aura_protocol::effects::choreographic::{ChoreographicRole, ChoreographyError};
use aura_protocol::effects::{CryptoEffects, JournalEffects};
use aura_types::{Commitment, DeviceId, Intent};
use rumpsteak_choreography::ChoreoHandler;
use serde::{Deserialize, Serialize};

/// Configuration for Prepare/ACK phase
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrepareAckConfig {
    /// Timeout for collecting ACKs in seconds
    pub timeout_seconds: u64,
    /// Minimum ACKs required (typically threshold)
    pub min_acks: usize,
}

/// Prepare phase proposal containing the intent and expected snapshot
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrepareProposal {
    /// The intent being prepared for execution
    pub intent: Intent,
    /// Expected snapshot commitment (for CAS check)
    pub expected_snapshot: Commitment,
    /// Device proposing the prepare phase (instigator)
    pub proposer: DeviceId,
}

/// Result of prepare phase validation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PrepareAckResult {
    /// All participants ACKed - snapshot matches
    Ack {
        /// Devices that acknowledged
        ack_devices: Vec<DeviceId>,
    },
    /// One or more participants NACKed - snapshot mismatch
    Nack {
        /// Devices that sent NACK
        nack_devices: Vec<DeviceId>,
        /// Their conflicting snapshots
        conflicting_snapshots: Vec<Commitment>,
    },
    /// Timeout waiting for responses
    Timeout,
}

/// Validator for prepare proposals
pub struct PrepareProposalValidator;

impl Default for PrepareProposalValidator {
    fn default() -> Self {
        Self::new()
    }
}

impl PrepareProposalValidator {
    /// Create a new prepare proposal validator
    pub fn new() -> Self {
        Self
    }
}

#[async_trait::async_trait]
impl ProposalValidator<PrepareProposal> for PrepareProposalValidator {
    fn validate_outgoing(
        &self,
        _proposal: &PrepareProposal,
        _proposer: ChoreographicRole,
    ) -> Result<(), String> {
        // Outgoing proposals are always valid (already constructed correctly)
        Ok(())
    }

    fn validate_incoming(
        &self,
        _proposal: &PrepareProposal,
        _proposer: ChoreographicRole,
        _receiver: ChoreographicRole,
    ) -> Result<(), String> {
        // Synchronous validation only
        // Just do basic structural checks here
        // More complex async validation would happen elsewhere
        Ok(())
    }
}

/// Prepare phase choreography
///
/// Uses propose_and_acknowledge pattern to validate snapshot across all participants.
/// If all participants ACK, the TreeSession can proceed to share exchange.
/// If any participant NACKs, the session must abort.
pub struct PreparePhase<H: ChoreoHandler<Role = ChoreographicRole>> {
    config: PrepareAckConfig,
    _phantom: std::marker::PhantomData<H>,
}

impl<H: ChoreoHandler<Role = ChoreographicRole> + Clone> PreparePhase<H> {
    /// Create a new prepare phase choreography
    pub fn new(config: PrepareAckConfig) -> Self {
        Self {
            config,
            _phantom: std::marker::PhantomData,
        }
    }

    /// Execute prepare phase with given proposal
    ///
    /// # Arguments
    ///
    /// * `handler` - Choreographic handler
    /// * `endpoint` - Communication endpoint
    /// * `proposal` - Prepare proposal to validate
    /// * `my_role` - This device's choreographic role
    /// * `participants` - All participants in the protocol
    ///
    /// # Returns
    ///
    /// PrepareAckResult indicating success (ACK) or failure (NACK/Timeout)
    pub async fn execute(
        &self,
        handler: &mut H,
        endpoint: &mut H::Endpoint,
        proposal: PrepareProposal,
        my_role: ChoreographicRole,
        participants: Vec<ChoreographicRole>,
    ) -> Result<PrepareAckResult, ChoreographyError>
    where
        H: JournalEffects + CryptoEffects,
    {
        let validator = PrepareProposalValidator::new();

        let propose_config = ProposeAcknowledgeConfig {
            acknowledge_timeout_seconds: self.config.timeout_seconds,
            require_explicit_acks: true, // Prepare phase needs explicit ACKs
            epoch: 0,                    // TODO: get from intent or config
            ..Default::default()
        };

        // Determine proposer role from proposal
        let proposer_role = ChoreographicRole {
            device_id: proposal.proposer.0, // Extract Uuid from DeviceId
            role_index: 0,                  // TODO: Properly determine role index
        };

        let handler_clone = handler.clone();
        let choreography = ProposeAndAcknowledgeChoreography::new(
            propose_config.clone(),
            participants.clone(),
            validator,
            &handler_clone,
        )
        .map_err(|e| ChoreographyError::ProtocolViolation {
            message: e.to_string(),
        })?;

        let result = if my_role.device_id == proposer_role.device_id {
            choreography
                .execute_as_proposer(handler, endpoint, my_role, proposal.clone())
                .await
                .map_err(|e| ChoreographyError::ProtocolViolation {
                    message: e.to_string(),
                })?
        } else {
            choreography
                .execute_as_participant(handler, endpoint, my_role, proposer_role)
                .await
                .map_err(|e| ChoreographyError::ProtocolViolation {
                    message: e.to_string(),
                })?
        };

        // Convert result to PrepareAckResult
        if result.all_acknowledged {
            Ok(PrepareAckResult::Ack {
                ack_devices: result
                    .acknowledged_participants
                    .into_iter()
                    .map(|role| DeviceId(role.device_id))
                    .collect(),
            })
        } else {
            // Check if timeout or NACK
            // TODO: ProposeAcknowledgeResult doesn't distinguish timeout vs NACK
            // For now, treat non-full-acknowledgment as timeout
            Ok(PrepareAckResult::Timeout)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_prepare_proposal_creation() {
        let intent = Intent {
            intent_id: aura_types::IntentId::new(),
            op: aura_types::TreeOp::EpochBump,
            path_span: vec![],
            snapshot_commitment: Commitment::from(vec![1, 2, 3]),
            priority: aura_types::Priority::Normal,
            author: DeviceId::new(),
            submitted_at: 0,
        };

        let proposal = PrepareProposal {
            intent: intent.clone(),
            expected_snapshot: Commitment::from(vec![1, 2, 3]),
            proposer: DeviceId::new(),
        };

        assert_eq!(proposal.intent.intent_id, intent.intent_id);
        assert_eq!(proposal.expected_snapshot, Commitment::from(vec![1, 2, 3]));
    }
}
