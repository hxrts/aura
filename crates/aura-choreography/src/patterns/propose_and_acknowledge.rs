//! Propose and Acknowledge choreographic pattern
//!
//! A simple choreography where a designated initiator role sends a piece of data
//! (the 'proposal') to all other participants, who then receive and implicitly
//! acknowledge it by proceeding to the next step. This is the simplest coordination
//! pattern and forms the building block for more complex protocols.
//!
//! Used extensively in:
//! - Protocol initialization and configuration distribution
//! - Epoch announcements and state transitions
//! - Leader-driven coordination in consensus protocols
//! - Configuration updates and parameter changes

use aura_protocol::effects::choreographic::ChoreographicRole;
use aura_types::effects::Effects;
use rumpsteak_choreography::{ChoreoHandler, ChoreographyError};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::fmt::Debug;
use std::time::Duration;

/// Configuration for propose and acknowledge operations
#[derive(Debug, Clone)]
pub struct ProposeAcknowledgeConfig {
    /// Timeout for acknowledgment phase
    pub acknowledge_timeout_seconds: u64,
    /// Whether to require explicit acknowledgments
    pub require_explicit_acks: bool,
    /// Maximum proposal size in bytes
    pub max_proposal_size: usize,
    /// Epoch for anti-replay protection
    pub epoch: u64,
    /// Whether to enable duplicate proposal detection
    pub detect_duplicate_proposals: bool,
}

impl Default for ProposeAcknowledgeConfig {
    fn default() -> Self {
        Self {
            acknowledge_timeout_seconds: 30,
            require_explicit_acks: false, // Implicit acknowledgment by default
            max_proposal_size: 1024 * 1024, // 1MB
            epoch: 0,
            detect_duplicate_proposals: true,
        }
    }
}

/// Result of propose and acknowledge operation
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(bound = "T: Clone + Serialize + DeserializeOwned + Debug + Send + Sync")]
pub struct ProposeAcknowledgeResult<T>
where
    T: Clone + Serialize + DeserializeOwned + Debug + Send + Sync,
{
    /// The proposal that was distributed
    pub proposal: T,
    /// Participants who received the proposal
    pub acknowledged_participants: Vec<ChoreographicRole>,
    /// Number of successful acknowledgments
    pub acknowledgment_count: usize,
    /// Whether all participants acknowledged
    pub all_acknowledged: bool,
    /// Total time taken for the operation
    pub duration_ms: u64,
    /// Success status
    pub success: bool,
}

/// Message types for the propose and acknowledge protocol
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(bound = "T: Clone + Serialize + DeserializeOwned + Debug + Send + Sync")]
pub enum ProposeAcknowledgeMessage<T>
where
    T: Clone + Serialize + DeserializeOwned + Debug + Send + Sync,
{
    /// Proposal message from initiator to participants
    Proposal {
        proposer: ChoreographicRole,
        proposal: T,
        sequence: u64,
        epoch: u64,
        proposal_hash: [u8; 32],
    },
    /// Explicit acknowledgment from participant to proposer
    Acknowledgment {
        participant: ChoreographicRole,
        proposal_hash: [u8; 32],
        epoch: u64,
    },
}

/// Trait for customizing proposal validation
pub trait ProposalValidator<T>
where
    T: Clone + Serialize + DeserializeOwned + Debug + Send + Sync,
{
    /// Validate a proposal before sending
    fn validate_outgoing(&self, proposal: &T, proposer: ChoreographicRole) -> Result<(), String>;

    /// Validate a received proposal
    fn validate_incoming(
        &self,
        proposal: &T,
        proposer: ChoreographicRole,
        receiver: ChoreographicRole,
    ) -> Result<(), String>;

    /// Check if this participant should acknowledge explicitly
    fn requires_explicit_ack(&self, _proposal: &T, _receiver: ChoreographicRole) -> bool {
        false
    }
}

/// Default validator that accepts all proposals
pub struct DefaultProposalValidator;

impl<T> ProposalValidator<T> for DefaultProposalValidator
where
    T: Clone + Serialize + DeserializeOwned + Debug + Send + Sync,
{
    fn validate_outgoing(&self, _proposal: &T, _proposer: ChoreographicRole) -> Result<(), String> {
        Ok(())
    }

    fn validate_incoming(
        &self,
        _proposal: &T,
        _proposer: ChoreographicRole,
        _receiver: ChoreographicRole,
    ) -> Result<(), String> {
        Ok(())
    }
}

/// Propose and Acknowledge choreography
pub struct ProposeAndAcknowledgeChoreography<T, V>
where
    T: Clone + Serialize + DeserializeOwned + Debug + Send + Sync,
    V: ProposalValidator<T>,
{
    config: ProposeAcknowledgeConfig,
    participants: Vec<ChoreographicRole>,
    validator: V,
    effects: Effects,
    operation_id: String,
    _phantom: std::marker::PhantomData<T>,
}

impl<T, V> ProposeAndAcknowledgeChoreography<T, V>
where
    T: Clone + Serialize + DeserializeOwned + Debug + Send + Sync,
    V: ProposalValidator<T>,
{
    /// Create new propose and acknowledge choreography
    pub fn new(
        config: ProposeAcknowledgeConfig,
        participants: Vec<ChoreographicRole>,
        validator: V,
        effects: Effects,
    ) -> Result<Self, ChoreographyError> {
        if participants.is_empty() {
            return Err(ChoreographyError::ProtocolViolation(
                "At least one participant required".to_string(),
            ));
        }

        let operation_id = uuid::Uuid::new_v4().to_string();

        Ok(Self {
            config,
            participants,
            validator,
            effects,
            operation_id,
            _phantom: std::marker::PhantomData,
        })
    }

    /// Execute as proposer (initiator) - sends proposal to all other participants
    pub async fn execute_as_proposer<H: ChoreoHandler<Role = ChoreographicRole>>(
        &self,
        handler: &mut H,
        endpoint: &mut H::Endpoint,
        my_role: ChoreographicRole,
        proposal: T,
    ) -> Result<ProposeAcknowledgeResult<T>, ChoreographyError> {
        let start_time = tokio::time::Instant::now();
        let timeout = Duration::from_secs(self.config.acknowledge_timeout_seconds);

        tracing::info!(
            operation_id = self.operation_id,
            proposer = ?my_role,
            participant_count = self.participants.len(),
            "Starting propose and acknowledge as proposer"
        );

        // Validate outgoing proposal
        self.validator
            .validate_outgoing(&proposal, my_role)
            .map_err(|e| {
                ChoreographyError::ProtocolViolation(format!("Proposal validation failed: {}", e))
            })?;

        // Check proposal size
        let proposal_size = bincode::serialize(&proposal)
            .map_err(|e| {
                ChoreographyError::ProtocolViolation(format!(
                    "Proposal serialization failed: {}",
                    e
                ))
            })?
            .len();

        if proposal_size > self.config.max_proposal_size {
            return Err(ChoreographyError::ProtocolViolation(format!(
                "Proposal too large: {} > {}",
                proposal_size, self.config.max_proposal_size
            )));
        }

        // Compute proposal hash
        let proposal_hash = self.compute_proposal_hash(&proposal)?;

        // Create proposal message
        let proposal_msg = ProposeAcknowledgeMessage::Proposal {
            proposer: my_role,
            proposal: proposal.clone(),
            sequence: 0, // Could be used for ordering in future
            epoch: self.config.epoch,
            proposal_hash,
        };

        // Phase 1: Send proposal to all other participants
        let mut sent_count = 0;
        let other_participants: Vec<_> = self
            .participants
            .iter()
            .filter(|p| **p != my_role)
            .copied()
            .collect();

        for participant in &other_participants {
            tracing::trace!(
                operation_id = self.operation_id,
                from = ?my_role,
                to = ?participant,
                "Sending proposal"
            );

            handler.send(endpoint, *participant, &proposal_msg).await?;
            sent_count += 1;
        }

        tracing::debug!(
            operation_id = self.operation_id,
            sent_count = sent_count,
            "Proposal phase complete"
        );

        // Phase 2: Collect acknowledgments (if required)
        let mut acknowledged_participants = vec![my_role]; // Include proposer

        if self.config.require_explicit_acks {
            for participant in &other_participants {
                // Check timeout
                if start_time.elapsed() > timeout {
                    tracing::warn!(
                        operation_id = self.operation_id,
                        acknowledged_count = acknowledged_participants.len() - 1, // Exclude proposer
                        expected_count = other_participants.len(),
                        "Acknowledgment timeout"
                    );
                    break;
                }

                tracing::trace!(
                    operation_id = self.operation_id,
                    from = ?participant,
                    to = ?my_role,
                    "Waiting for acknowledgment"
                );

                let received: ProposeAcknowledgeMessage<T> =
                    handler.recv(endpoint, *participant).await?;

                if let ProposeAcknowledgeMessage::Acknowledgment {
                    participant: sender,
                    proposal_hash: ack_hash,
                    epoch,
                } = received
                {
                    // Verify epoch
                    if epoch != self.config.epoch {
                        tracing::warn!(
                            operation_id = self.operation_id,
                            expected_epoch = self.config.epoch,
                            received_epoch = epoch,
                            sender = ?sender,
                            "Epoch mismatch in acknowledgment"
                        );
                        continue; // Skip this ack but don't fail entirely
                    }

                    // Verify sender matches
                    if sender != *participant {
                        tracing::warn!(
                            operation_id = self.operation_id,
                            expected_sender = ?participant,
                            claimed_sender = ?sender,
                            "Sender mismatch in acknowledgment"
                        );
                        continue; // Skip this ack but don't fail entirely
                    }

                    // Verify proposal hash
                    if ack_hash != proposal_hash {
                        tracing::warn!(
                            operation_id = self.operation_id,
                            sender = ?sender,
                            "Proposal hash mismatch in acknowledgment"
                        );
                        continue; // Skip this ack but don't fail entirely
                    }

                    acknowledged_participants.push(*participant);

                    tracing::trace!(
                        operation_id = self.operation_id,
                        sender = ?participant,
                        progress = format!("{}/{}", acknowledged_participants.len() - 1, other_participants.len()),
                        "Acknowledgment received"
                    );
                } else {
                    tracing::warn!(
                        operation_id = self.operation_id,
                        sender = ?participant,
                        "Expected acknowledgment message but received something else"
                    );
                }
            }
        } else {
            // For implicit acknowledgment, assume all participants acknowledge
            acknowledged_participants.extend(other_participants.clone());
        }

        let duration = start_time.elapsed();
        let all_acknowledged = acknowledged_participants.len() == self.participants.len();
        let success = all_acknowledged || !self.config.require_explicit_acks;

        tracing::info!(
            operation_id = self.operation_id,
            proposer = ?my_role,
            acknowledged_count = acknowledged_participants.len() - 1, // Exclude proposer
            expected_count = other_participants.len(),
            all_acknowledged = all_acknowledged,
            duration_ms = duration.as_millis(),
            success = success,
            "Propose and acknowledge completed as proposer"
        );

        Ok(ProposeAcknowledgeResult {
            proposal,
            acknowledged_participants: acknowledged_participants.clone(),
            acknowledgment_count: acknowledged_participants.len() - 1, // Exclude proposer
            all_acknowledged,
            duration_ms: duration.as_millis() as u64,
            success,
        })
    }

    /// Execute as participant (receiver) - receives proposal and optionally acknowledges
    pub async fn execute_as_participant<H: ChoreoHandler<Role = ChoreographicRole>>(
        &self,
        handler: &mut H,
        endpoint: &mut H::Endpoint,
        my_role: ChoreographicRole,
        proposer: ChoreographicRole,
    ) -> Result<ProposeAcknowledgeResult<T>, ChoreographyError> {
        let start_time = tokio::time::Instant::now();
        let _timeout = Duration::from_secs(self.config.acknowledge_timeout_seconds);

        tracing::info!(
            operation_id = self.operation_id,
            participant = ?my_role,
            proposer = ?proposer,
            "Starting propose and acknowledge as participant"
        );

        // Phase 1: Receive proposal
        tracing::trace!(
            operation_id = self.operation_id,
            from = ?proposer,
            to = ?my_role,
            "Waiting for proposal"
        );

        let received: ProposeAcknowledgeMessage<T> = handler.recv(endpoint, proposer).await?;

        let proposal = if let ProposeAcknowledgeMessage::Proposal {
            proposer: sender,
            proposal,
            sequence: _,
            epoch,
            proposal_hash,
        } = received
        {
            // Verify epoch
            if epoch != self.config.epoch {
                return Err(ChoreographyError::ProtocolViolation(format!(
                    "Epoch mismatch: expected {}, got {}",
                    self.config.epoch, epoch
                )));
            }

            // Verify sender matches expected proposer
            if sender != proposer {
                return Err(ChoreographyError::ProtocolViolation(format!(
                    "Proposer mismatch: expected {:?}, got {:?}",
                    proposer, sender
                )));
            }

            // Verify proposal integrity
            let expected_hash = self.compute_proposal_hash(&proposal)?;
            if proposal_hash != expected_hash {
                return Err(ChoreographyError::ProtocolViolation(
                    "Proposal integrity check failed".to_string(),
                ));
            }

            // Validate incoming proposal
            self.validator
                .validate_incoming(&proposal, proposer, my_role)
                .map_err(|e| {
                    ChoreographyError::ProtocolViolation(format!(
                        "Incoming proposal validation failed: {}",
                        e
                    ))
                })?;

            tracing::debug!(
                operation_id = self.operation_id,
                participant = ?my_role,
                proposer = ?proposer,
                "Proposal received and validated"
            );

            proposal
        } else {
            return Err(ChoreographyError::ProtocolViolation(
                "Expected proposal message".to_string(),
            ));
        };

        // Phase 2: Send acknowledgment (if required)
        if self.config.require_explicit_acks
            || self.validator.requires_explicit_ack(&proposal, my_role)
        {
            let proposal_hash = self.compute_proposal_hash(&proposal)?;
            let ack_msg: ProposeAcknowledgeMessage<T> = ProposeAcknowledgeMessage::Acknowledgment {
                participant: my_role,
                proposal_hash,
                epoch: self.config.epoch,
            };

            tracing::trace!(
                operation_id = self.operation_id,
                from = ?my_role,
                to = ?proposer,
                "Sending acknowledgment"
            );

            handler.send(endpoint, proposer, &ack_msg).await?;

            tracing::debug!(
                operation_id = self.operation_id,
                participant = ?my_role,
                proposer = ?proposer,
                "Acknowledgment sent"
            );
        }

        let duration = start_time.elapsed();

        tracing::info!(
            operation_id = self.operation_id,
            participant = ?my_role,
            proposer = ?proposer,
            duration_ms = duration.as_millis(),
            "Propose and acknowledge completed as participant"
        );

        Ok(ProposeAcknowledgeResult {
            proposal,
            acknowledged_participants: vec![my_role], // Only track self
            acknowledgment_count: 1,
            all_acknowledged: true, // From participant perspective, they acknowledged
            duration_ms: duration.as_millis() as u64,
            success: true,
        })
    }

    fn compute_proposal_hash(&self, proposal: &T) -> Result<[u8; 32], ChoreographyError> {
        let serialized = bincode::serialize(proposal).map_err(|e| {
            ChoreographyError::ProtocolViolation(format!("Proposal serialization failed: {}", e))
        })?;
        Ok(self.effects.blake3_hash(&serialized))
    }
}

/// Convenience function for simple proposal as proposer
pub async fn propose_to_participants<T, H>(
    handler: &mut H,
    endpoint: &mut H::Endpoint,
    participants: Vec<ChoreographicRole>,
    my_role: ChoreographicRole,
    proposal: T,
    effects: Effects,
) -> Result<T, ChoreographyError>
where
    T: Clone + Serialize + DeserializeOwned + Debug + Send + Sync,
    H: ChoreoHandler<Role = ChoreographicRole>,
{
    let config = ProposeAcknowledgeConfig::default();
    let validator = DefaultProposalValidator;

    let choreography =
        ProposeAndAcknowledgeChoreography::new(config, participants, validator, effects)?;

    let result = choreography
        .execute_as_proposer(handler, endpoint, my_role, proposal)
        .await?;
    Ok(result.proposal)
}

/// Convenience function for simple proposal as participant
pub async fn receive_proposal_from<T, H>(
    handler: &mut H,
    endpoint: &mut H::Endpoint,
    participants: Vec<ChoreographicRole>,
    my_role: ChoreographicRole,
    proposer: ChoreographicRole,
    effects: Effects,
) -> Result<T, ChoreographyError>
where
    T: Clone + Serialize + DeserializeOwned + Debug + Send + Sync,
    H: ChoreoHandler<Role = ChoreographicRole>,
{
    let config = ProposeAcknowledgeConfig::default();
    let validator = DefaultProposalValidator;

    let choreography =
        ProposeAndAcknowledgeChoreography::new(config, participants, validator, effects)?;

    let result = choreography
        .execute_as_participant(handler, endpoint, my_role, proposer)
        .await?;
    Ok(result.proposal)
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
    struct TestProposal {
        content: String,
        value: u32,
        config: Vec<u8>,
    }

    #[tokio::test]
    async fn test_propose_acknowledge_creation() {
        let effects = Effects::test(42);
        let participants = vec![
            ChoreographicRole {
                device_id: Uuid::new_v4(),
                role_index: 0,
            },
            ChoreographicRole {
                device_id: Uuid::new_v4(),
                role_index: 1,
            },
        ];

        let config = ProposeAcknowledgeConfig::default();
        let validator = DefaultProposalValidator;

        let choreography = ProposeAndAcknowledgeChoreography::<TestProposal, _>::new(
            config,
            participants,
            validator,
            effects,
        );

        assert!(choreography.is_ok());
    }

    #[test]
    fn test_proposal_validator() {
        let validator = DefaultProposalValidator;
        let role = ChoreographicRole {
            device_id: Uuid::new_v4(),
            role_index: 0,
        };
        let proposal = TestProposal {
            content: "test proposal".to_string(),
            value: 42,
            config: vec![1, 2, 3],
        };

        assert!(validator.validate_outgoing(&proposal, role).is_ok());
        assert!(validator.validate_incoming(&proposal, role, role).is_ok());
        assert!(!validator.requires_explicit_ack(&proposal, role));
    }

    #[test]
    fn test_config_defaults() {
        let config = ProposeAcknowledgeConfig::default();

        assert_eq!(config.acknowledge_timeout_seconds, 30);
        assert!(!config.require_explicit_acks);
        assert_eq!(config.max_proposal_size, 1024 * 1024);
        assert_eq!(config.epoch, 0);
        assert!(config.detect_duplicate_proposals);
    }
}
