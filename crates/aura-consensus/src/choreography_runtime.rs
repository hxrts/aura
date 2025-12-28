//! Choreographic runtime for Aura Consensus.
//!
//! This module provides an executable path for the consensus choreography
//! using the ChoreographicEffects interface and the generated session types.

use crate::messages::ConsensusMessage;
use aura_core::effects::{ChoreographicEffects, ChoreographicRole, ChoreographyError};
use aura_core::util::serialization::{from_slice, to_vec};

/// Result from coordinator execution.
#[derive(Debug, Clone)]
pub struct ConsensusCoordinatorResult {
    pub nonce_commits: Vec<ConsensusMessage>,
    pub sign_shares: Vec<ConsensusMessage>,
}

fn encode_message(message: &ConsensusMessage) -> Result<Vec<u8>, ChoreographyError> {
    to_vec(message).map_err(|e| ChoreographyError::SerializationFailed {
        reason: e.to_string(),
    })
}

fn decode_message(payload: &[u8]) -> Result<ConsensusMessage, ChoreographyError> {
    from_slice(payload).map_err(|e| ChoreographyError::DeserializationFailed {
        reason: e.to_string(),
    })
}

/// Execute the coordinator side of the consensus choreography.
pub async fn run_coordinator<E: ChoreographicEffects>(
    effects: &E,
    witnesses: &[ChoreographicRole],
    execute: ConsensusMessage,
    sign_request: ConsensusMessage,
    result: ConsensusMessage,
) -> Result<ConsensusCoordinatorResult, ChoreographyError> {
    let execute_payload = encode_message(&execute)?;
    for witness in witnesses {
        effects
            .send_to_role_bytes(*witness, execute_payload.clone())
            .await?;
    }

    let mut nonce_commits = Vec::with_capacity(witnesses.len());
    for witness in witnesses {
        let payload = effects.receive_from_role_bytes(*witness).await?;
        nonce_commits.push(decode_message(&payload)?);
    }

    let sign_payload = encode_message(&sign_request)?;
    for witness in witnesses {
        effects
            .send_to_role_bytes(*witness, sign_payload.clone())
            .await?;
    }

    let mut sign_shares = Vec::with_capacity(witnesses.len());
    for witness in witnesses {
        let payload = effects.receive_from_role_bytes(*witness).await?;
        sign_shares.push(decode_message(&payload)?);
    }

    let result_payload = encode_message(&result)?;
    for witness in witnesses {
        effects
            .send_to_role_bytes(*witness, result_payload.clone())
            .await?;
    }

    Ok(ConsensusCoordinatorResult {
        nonce_commits,
        sign_shares,
    })
}

/// Execute the witness side of the consensus choreography.
pub async fn run_witness<E: ChoreographicEffects>(
    effects: &E,
    coordinator: ChoreographicRole,
    nonce_commit: ConsensusMessage,
    sign_share: ConsensusMessage,
) -> Result<ConsensusMessage, ChoreographyError> {
    let execute_payload = effects.receive_from_role_bytes(coordinator).await?;
    let _execute = decode_message(&execute_payload)?;

    let nonce_payload = encode_message(&nonce_commit)?;
    effects
        .send_to_role_bytes(coordinator, nonce_payload)
        .await?;

    let sign_payload = effects.receive_from_role_bytes(coordinator).await?;
    let _sign_request = decode_message(&sign_payload)?;

    let share_payload = encode_message(&sign_share)?;
    effects
        .send_to_role_bytes(coordinator, share_payload)
        .await?;

    let result_payload = effects.receive_from_role_bytes(coordinator).await?;
    decode_message(&result_payload)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{CommitFact, ConsensusId};
    use aura_core::effects::{ChoreographyEvent, ChoreographyMetrics};
    use aura_core::frost::{NonceCommitment, PartialSignature, ThresholdSignature};
    use aura_core::time::{PhysicalTime, ProvenancedTime, TimeStamp};
    use aura_core::{AuthorityId, Hash32};
    use futures::future::yield_now;
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};
    use uuid::Uuid;

    #[derive(Default)]
    struct SharedBus {
        queues: Mutex<HashMap<(ChoreographicRole, ChoreographicRole), Vec<Vec<u8>>>>,
        roles: Mutex<Vec<ChoreographicRole>>,
    }

    struct BusHandler {
        role: ChoreographicRole,
        bus: Arc<SharedBus>,
    }

    impl BusHandler {
        fn new(role: ChoreographicRole, bus: Arc<SharedBus>) -> Self {
            Self { role, bus }
        }
    }

    #[async_trait::async_trait]
    impl ChoreographicEffects for BusHandler {
        async fn send_to_role_bytes(
            &self,
            role: ChoreographicRole,
            message: Vec<u8>,
        ) -> Result<(), ChoreographyError> {
            let mut queues =
                self.bus
                    .queues
                    .lock()
                    .map_err(|_| ChoreographyError::InternalError {
                        message: "shared bus lock poisoned".to_string(),
                    })?;
            queues.entry((self.role, role)).or_default().push(message);
            Ok(())
        }

        async fn receive_from_role_bytes(
            &self,
            role: ChoreographicRole,
        ) -> Result<Vec<u8>, ChoreographyError> {
            for _ in 0..200 {
                {
                    let mut queues = self
                        .bus
                        .queues
                        .lock()
                        .map_err(|_| ChoreographyError::InternalError {
                            message: "shared bus lock poisoned".to_string(),
                        })?;
                    if let Some(queue) = queues.get_mut(&(role, self.role)) {
                        if let Some(payload) = queue.pop() {
                            return Ok(payload);
                        }
                    }
                }
                yield_now().await;
            }
            Err(ChoreographyError::CommunicationTimeout {
                role,
                timeout_ms: 1,
            })
        }

        async fn broadcast_bytes(&self, message: Vec<u8>) -> Result<(), ChoreographyError> {
            let roles = self
                .bus
                .roles
                .lock()
                .map_err(|_| ChoreographyError::InternalError {
                    message: "shared bus lock poisoned".to_string(),
                })?
                .clone();
            for role in roles {
                if role != self.role {
                    self.send_to_role_bytes(role, message.clone()).await?;
                }
            }
            Ok(())
        }

        fn current_role(&self) -> ChoreographicRole {
            self.role
        }

        fn all_roles(&self) -> Vec<ChoreographicRole> {
            self.bus
                .roles
                .lock()
                .expect("shared bus lock should not be poisoned")
                .clone()
        }

        async fn is_role_active(&self, role: ChoreographicRole) -> bool {
            self.all_roles().contains(&role)
        }

        async fn start_session(
            &self,
            _session_id: Uuid,
            roles: Vec<ChoreographicRole>,
        ) -> Result<(), ChoreographyError> {
            let mut guard =
                self.bus
                    .roles
                    .lock()
                    .map_err(|_| ChoreographyError::InternalError {
                        message: "shared bus lock poisoned".to_string(),
                    })?;
            *guard = roles;
            Ok(())
        }

        async fn end_session(&self) -> Result<(), ChoreographyError> {
            self.bus
                .roles
                .lock()
                .map_err(|_| ChoreographyError::InternalError {
                    message: "shared bus lock poisoned".to_string(),
                })?
                .clear();
            Ok(())
        }

        async fn emit_choreo_event(
            &self,
            _event: ChoreographyEvent,
        ) -> Result<(), ChoreographyError> {
            Ok(())
        }

        async fn set_timeout(&self, _timeout_ms: u64) {}

        async fn get_metrics(&self) -> ChoreographyMetrics {
            ChoreographyMetrics {
                messages_sent: 0,
                messages_received: 0,
                avg_latency_ms: 0.0,
                timeout_count: 0,
                retry_count: 0,
                total_duration_ms: 0,
            }
        }
    }

    #[tokio::test]
    async fn consensus_choreography_round_trip() {
        let coordinator = ChoreographicRole::new(Uuid::from_bytes([1u8; 16]), 0);
        let witness = ChoreographicRole::new(Uuid::from_bytes([2u8; 16]), 1);
        let bus = Arc::new(SharedBus::default());

        {
            let mut roles = bus.roles.lock().unwrap();
            roles.push(coordinator);
            roles.push(witness);
        }

        let coordinator_handler = BusHandler::new(coordinator, bus.clone());
        let witness_handler = BusHandler::new(witness, bus.clone());

        let consensus_id = ConsensusId::new(Hash32::default(), Hash32([1u8; 32]), 7);
        let execute = ConsensusMessage::Execute {
            consensus_id,
            prestate_hash: Hash32::default(),
            operation_hash: Hash32([2u8; 32]),
            operation_bytes: vec![1, 2, 3],
            cached_commitments: None,
        };

        let nonce_commit = ConsensusMessage::NonceCommit {
            consensus_id,
            commitment: NonceCommitment {
                signer: 1,
                commitment: vec![0u8; 32],
            },
        };

        let sign_request = ConsensusMessage::SignRequest {
            consensus_id,
            aggregated_nonces: vec![NonceCommitment {
                signer: 1,
                commitment: vec![0u8; 32],
            }],
        };

        let sign_share = ConsensusMessage::SignShare {
            consensus_id,
            share: PartialSignature {
                signer: 1,
                signature: vec![0u8; 32],
            },
            next_commitment: None,
            epoch: aura_core::types::Epoch::new(1),
        };

        let commit_fact = CommitFact::new(
            consensus_id,
            Hash32::default(),
            Hash32([3u8; 32]),
            vec![4, 5, 6],
            ThresholdSignature::new(vec![0u8; 64], vec![1]),
            None,
            vec![AuthorityId::new_from_entropy([9u8; 32])],
            1,
            false,
            ProvenancedTime {
                stamp: TimeStamp::PhysicalClock(PhysicalTime {
                    ts_ms: 1234,
                    uncertainty: None,
                }),
                proofs: vec![],
                origin: None,
            },
        );

        let result = ConsensusMessage::ConsensusResult { commit_fact };

        let witnesses = [witness];
        let coordinator_task = run_coordinator(
            &coordinator_handler,
            &witnesses,
            execute.clone(),
            sign_request.clone(),
            result.clone(),
        );
        let witness_task = run_witness(
            &witness_handler,
            coordinator,
            nonce_commit.clone(),
            sign_share.clone(),
        );

        let (coord_result, witness_result) = futures::join!(coordinator_task, witness_task);

        let coord_result = coord_result.expect("coordinator should succeed");
        let witness_result = witness_result.expect("witness should succeed");

        assert_eq!(coord_result.nonce_commits.len(), 1);
        assert_eq!(coord_result.sign_shares.len(), 1);
        assert!(matches!(
            witness_result,
            ConsensusMessage::ConsensusResult { .. }
        ));
    }
}
