//! Decentralized lottery for coordinator selection

use aura_protocol::effects::choreographic::ChoreographicRole;
use rumpsteak_choreography::{ChoreoHandler, ChoreographyError};
use serde::{Deserialize, Serialize};

/// Lottery protocol messages
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum LotteryMessage {
    /// Random value commitment
    RandomCommitment {
        commitment: [u8; 32],
        participant_id: usize,
    },
    /// Random value reveal
    RandomReveal {
        value: [u8; 32],
        participant_id: usize,
    },
    /// Selected coordinator announcement
    CoordinatorSelected { coordinator_id: usize },
}

/// Decentralized lottery for fair coordinator selection
pub struct DecentralizedLottery {
    participants: Vec<ChoreographicRole>,
}

impl DecentralizedLottery {
    pub fn new(participants: Vec<ChoreographicRole>) -> Self {
        Self { participants }
    }

    /// Execute lottery to select coordinator
    pub async fn execute<H: ChoreoHandler<Role = ChoreographicRole>>(
        &self,
        handler: &mut H,
        endpoint: &mut H::Endpoint,
        my_role: ChoreographicRole,
    ) -> Result<ChoreographicRole, ChoreographyError> {
        let n = self.participants.len();

        // Phase 1: Commit to random values
        let effects = aura_protocol::effects::Effects::production();
        let my_random = effects.random_bytes_array::<32>();
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
                        // TODO In production, verify against commitment
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
        // Placeholder - TODO in production use blake3
        let mut result = [0u8; 32];
        result[..data.len().min(32)].copy_from_slice(&data[..data.len().min(32)]);
        result
    }

    fn combine_randomness(&self, reveals: &[(usize, [u8; 32])]) -> [u8; 32] {
        // Placeholder - TODO in production XOR all values
        let mut result = [0u8; 32];
        for (_, value) in reveals {
            for i in 0..32 {
                result[i] ^= value[i];
            }
        }
        result
    }
}
