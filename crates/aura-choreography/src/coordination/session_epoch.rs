//! Session epoch monitoring and coordination choreographies

use aura_protocol::effects::choreographic::ChoreographicRole;
use rumpsteak_choreography::{ChoreoHandler, ChoreographyError};
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Messages for session epoch coordination
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EpochMessage {
    /// Current epoch query
    EpochQuery { participant_id: usize },

    /// Epoch response
    EpochResponse {
        participant_id: usize,
        current_epoch: u64,
    },

    /// Propose epoch bump
    ProposeEpochBump {
        participant_id: usize,
        new_epoch: u64,
        reason: String,
    },

    /// Acknowledge epoch bump
    AckEpochBump {
        participant_id: usize,
        new_epoch: u64,
    },
}

/// Session epoch monitoring choreography
pub struct SessionEpochMonitor {
    participants: Vec<ChoreographicRole>,
    check_interval: Duration,
}

impl SessionEpochMonitor {
    pub fn new(participants: Vec<ChoreographicRole>, check_interval: Duration) -> Self {
        Self {
            participants,
            check_interval,
        }
    }

    /// Monitor session epochs across participants
    pub async fn monitor<H: ChoreoHandler<Role = ChoreographicRole>>(
        &self,
        handler: &mut H,
        endpoint: &mut H::Endpoint,
        my_role: ChoreographicRole,
        local_epoch: u64,
    ) -> Result<u64, ChoreographyError> {
        // Query all participants for their current epoch
        let query_msg = EpochMessage::EpochQuery {
            participant_id: my_role.role_index,
        };

        // Send epoch query to all participants
        for participant in &self.participants {
            if *participant != my_role {
                handler.send(endpoint, *participant, &query_msg).await?;
            }
        }

        // Collect epoch responses - use provided local epoch
        let mut epochs = vec![(my_role.role_index, local_epoch)];

        for participant in &self.participants {
            if *participant != my_role {
                // Use timeout to handle unresponsive participants
                // Note: with_timeout is designed for timing out the recv operation itself
                match handler.recv::<EpochMessage>(endpoint, *participant).await {
                    Ok(EpochMessage::EpochResponse {
                        participant_id,
                        current_epoch,
                    }) => {
                        epochs.push((participant_id, current_epoch));
                    }
                    Ok(_) => {
                        return Err(ChoreographyError::ProtocolViolation(
                            "Unexpected message type".to_string(),
                        ));
                    }
                    Err(_) => {
                        // Participant timeout - they may have failed
                        continue;
                    }
                }
            }
        }

        // Find the maximum epoch
        let max_epoch = epochs.iter().map(|(_, epoch)| *epoch).max().unwrap_or(0);

        Ok(max_epoch)
    }
}

/// Epoch bump choreography for failure recovery
pub struct EpochBumpChoreography {
    participants: Vec<ChoreographicRole>,
    required_confirmations: usize,
}

impl EpochBumpChoreography {
    pub fn new(participants: Vec<ChoreographicRole>, required_confirmations: usize) -> Self {
        Self {
            participants,
            required_confirmations,
        }
    }

    /// Execute coordinated epoch bump
    pub async fn bump_epoch<H: ChoreoHandler<Role = ChoreographicRole>>(
        &self,
        handler: &mut H,
        endpoint: &mut H::Endpoint,
        my_role: ChoreographicRole,
        current_epoch: u64,
        reason: String,
    ) -> Result<u64, ChoreographyError> {
        // Calculate new epoch
        let new_epoch = current_epoch + 1;

        // Propose epoch bump to all participants
        let propose_msg = EpochMessage::ProposeEpochBump {
            participant_id: my_role.role_index,
            new_epoch,
            reason,
        };

        for participant in &self.participants {
            if *participant != my_role {
                handler.send(endpoint, *participant, &propose_msg).await?;
            }
        }

        // Collect acknowledgments
        let mut acks = 1; // Count self

        for participant in &self.participants {
            if *participant != my_role {
                match handler.recv::<EpochMessage>(endpoint, *participant).await {
                    Ok(EpochMessage::AckEpochBump {
                        participant_id: _,
                        new_epoch: acked_epoch,
                    }) => {
                        if acked_epoch == new_epoch {
                            acks += 1;
                        }
                    }
                    _ => continue, // Timeout or wrong message type
                }
            }
        }

        // Check if we have enough acknowledgments
        if acks >= self.required_confirmations {
            Ok(new_epoch)
        } else {
            Err(ChoreographyError::ProtocolViolation(format!(
                "Insufficient acknowledgments for epoch bump: {} of {} required",
                acks, self.required_confirmations
            )))
        }
    }
}
