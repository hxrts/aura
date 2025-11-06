//! Decentralized lottery for coordinator selection

use aura_protocol::effects::choreographic::ChoreographicRole;
use rumpsteak_choreography::{ChoreoHandler, ChoreographyError};
use serde::{Deserialize, Serialize};

/// Lottery protocol messages
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum LotteryMessage {
    /// Random value commitment
    RandomCommitment {
        /// Commitment hash of the random value
        commitment: [u8; 32],
        /// ID of the participant
        participant_id: usize,
    },
    /// Random value reveal
    RandomReveal {
        /// The revealed random value
        value: [u8; 32],
        /// ID of the participant
        participant_id: usize,
    },
    /// Selected coordinator announcement
    CoordinatorSelected {
        /// ID of the selected coordinator
        coordinator_id: usize,
    },
}

/// Decentralized lottery for fair coordinator selection
pub struct DecentralizedLottery {
    /// List of participants involved in the lottery
    participants: Vec<ChoreographicRole>,
}

impl DecentralizedLottery {
    /// Create a new decentralized lottery instance
    ///
    /// # Arguments
    ///
    /// * `participants` - List of choreographic roles participating in the lottery
    pub fn new(participants: Vec<ChoreographicRole>) -> Self {
        Self { participants }
    }

    /// Execute lottery to select coordinator
    ///
    /// # Arguments
    ///
    /// * `handler` - Choreography handler for message passing
    /// * `endpoint` - Handler endpoint for network communication
    /// * `my_role` - This participant's choreographic role
    pub async fn execute<H: ChoreoHandler<Role = ChoreographicRole>>(
        &self,
        handler: &mut H,
        endpoint: &mut H::Endpoint,
        my_role: ChoreographicRole,
    ) -> Result<ChoreographicRole, ChoreographyError> {
        let n = self.participants.len();

        // Phase 1: Commit to random values
        use aura_protocol::effects::RandomEffects;
        let my_random: [u8; 32] = aura_protocol::effects::ProductionRandomEffects
            .random_bytes(32)
            .try_into()
            .unwrap();
        let my_commitment = self.hash(&my_random);

        let commit_msg = LotteryMessage::RandomCommitment {
            commitment: my_commitment,
            participant_id: my_role.role_index,
        };

        // Broadcast commitment
        for participant in &self.participants {
            if *participant != my_role {
                handler.send(endpoint, *participant, &commit_msg).await?;
            }
        }

        // Collect all commitments
        let mut commitments = vec![(my_role.role_index, my_commitment)];
        for participant in &self.participants {
            if *participant != my_role {
                match handler
                    .recv::<LotteryMessage>(endpoint, *participant)
                    .await?
                {
                    LotteryMessage::RandomCommitment {
                        commitment,
                        participant_id,
                    } => {
                        commitments.push((participant_id, commitment));
                    }
                    _ => {
                        return Err(ChoreographyError::Transport(
                            "Unexpected message".to_string(),
                        ))
                    }
                }
            }
        }

        // Phase 2: Reveal random values
        let reveal_msg = LotteryMessage::RandomReveal {
            value: my_random,
            participant_id: my_role.role_index,
        };

        // Broadcast reveal
        for participant in &self.participants {
            if *participant != my_role {
                handler.send(endpoint, *participant, &reveal_msg).await?;
            }
        }

        // Collect all reveals
        let mut reveals = vec![(my_role.role_index, my_random)];
        for participant in &self.participants {
            if *participant != my_role {
                match handler
                    .recv::<LotteryMessage>(endpoint, *participant)
                    .await?
                {
                    LotteryMessage::RandomReveal {
                        value,
                        participant_id,
                    } => {
                        // Verify reveal matches commitment
                        let expected_commitment = commitments
                            .iter()
                            .find(|(id, _)| *id == participant_id)
                            .map(|(_, c)| *c)
                            .ok_or_else(|| {
                                ChoreographyError::Transport(format!(
                                    "No commitment found for participant {}",
                                    participant_id
                                ))
                            })?;

                        let actual_commitment = self.hash(&value);
                        if actual_commitment != expected_commitment {
                            return Err(ChoreographyError::Transport(format!(
                                "Commitment verification failed for participant {}",
                                participant_id
                            )));
                        }

                        reveals.push((participant_id, value));
                    }
                    _ => {
                        return Err(ChoreographyError::Transport(
                            "Unexpected message".to_string(),
                        ))
                    }
                }
            }
        }

        // Phase 3: Deterministic selection
        let combined = self.combine_randomness(&reveals);
        let selected_index = (combined[0] as usize) % n;
        // Find the participant with the selected index
        let selected_role = self
            .participants
            .iter()
            .find(|p| p.role_index == selected_index)
            .cloned()
            .unwrap_or(self.participants[0]);

        // Phase 4: Announce result
        let announce_msg = LotteryMessage::CoordinatorSelected {
            coordinator_id: selected_index,
        };

        // Everyone broadcasts their computed result
        for participant in &self.participants {
            if *participant != my_role {
                handler.send(endpoint, *participant, &announce_msg).await?;
            }
        }

        // Verify consensus
        for participant in &self.participants {
            if *participant != my_role {
                match handler
                    .recv::<LotteryMessage>(endpoint, *participant)
                    .await?
                {
                    LotteryMessage::CoordinatorSelected { coordinator_id } => {
                        if coordinator_id != selected_index {
                            return Err(ChoreographyError::Transport(
                                "Lottery consensus failed".to_string(),
                            ));
                        }
                    }
                    _ => {
                        return Err(ChoreographyError::Transport(
                            "Unexpected message".to_string(),
                        ))
                    }
                }
            }
        }

        Ok(selected_role)
    }

    fn hash(&self, data: &[u8]) -> [u8; 32] {
        *blake3::hash(data).as_bytes()
    }

    fn combine_randomness(&self, reveals: &[(usize, [u8; 32])]) -> [u8; 32] {
        let mut result = [0u8; 32];
        for (_, value) in reveals {
            for i in 0..32 {
                result[i] ^= value[i];
            }
        }
        result
    }
}
