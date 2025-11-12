//! Threshold Signature Ceremony Choreography
//!
//! This module implements choreographic protocols for coordinated
//! FROST threshold signing of tree operations with privacy-preserving
//! properties using rumpsteak-aura DSL.
//!
//! ## Protocol Flow
//!
//! ### Phase 1: Initialization
//! 1. Coordinator → Signers[N]: SignRequest { message, binding_context }
//!
//! ### Phase 2: Nonce Commitment
//! 2. Signers[N] → Coordinator: NonceCommit { commitment }
//!
//! ### Phase 3: Signing
//! 3. Coordinator → Signers[N]: ChallengeRequest
//! 4. Signers[N] → Coordinator: PartialSig { signature }
//!
//! ### Phase 4: Finalization
//! 5. Coordinator → Signers[N] + Observers[M]: AttestedResult { op }
//!
//! ## Privacy Properties
//!
//! - No signer identities revealed in AttestedOp
//! - Only signature count revealed (threshold satisfied)
//! - Parent binding prevents replay attacks

use crate::effects::CryptoEffects;
use aura_core::effects::RandomEffects;
use aura_core::tree::Epoch;
use aura_core::{AttestedOp, DeviceId, Hash32, SessionId, TreeOp};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Threshold ceremony configuration
#[derive(Debug, Clone)]
pub struct CeremonyConfig {
    /// Participating signers (includes coordinator)
    pub participants: Vec<DeviceId>,
    /// Observer device IDs (receive result but don't sign)
    pub observers: Vec<DeviceId>,
    /// Threshold required (m-of-n)
    pub threshold: u16,
    /// Tree operation to sign
    pub operation: TreeOp,
    /// Parent binding context for replay prevention
    pub parent_binding: ParentBinding,
}

/// Parent binding context for replay prevention
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParentBinding {
    /// Parent epoch
    pub parent_epoch: Epoch,
    /// Parent commitment
    pub parent_commitment: Hash32,
    /// Policy hash at signing time
    pub policy_hash: Hash32,
}

impl ParentBinding {
    pub fn new(parent_epoch: Epoch, parent_commitment: Hash32, policy_hash: Hash32) -> Self {
        Self {
            parent_epoch,
            parent_commitment,
            policy_hash,
        }
    }

    /// Create binding message for signing
    pub fn binding_message(&self, op_bytes: &[u8]) -> Vec<u8> {
        let mut msg = Vec::new();
        msg.extend_from_slice(b"TREE_OP_SIG");
        msg.extend_from_slice(&self.parent_epoch.to_le_bytes());
        msg.extend_from_slice(self.parent_commitment.as_ref());
        msg.extend_from_slice(self.policy_hash.as_ref());
        msg.extend_from_slice(op_bytes);
        msg
    }
}

/// Threshold ceremony result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CeremonyResult {
    /// Resulting attested operation
    pub attested_op: Option<AttestedOp>,
    /// Number of partial signatures collected
    pub signatures_collected: u16,
    /// Whether ceremony completed successfully
    pub success: bool,
}

/// Ceremony error types
#[derive(Debug, thiserror::Error)]
pub enum CeremonyError {
    #[error("Invalid configuration: {0}")]
    InvalidConfig(String),
    #[error("Communication error: {0}")]
    Communication(String),
    #[error("Ceremony failed: {0}")]
    CeremonyFailed(String),
    #[error("Threshold not met: got {got}, need {need}")]
    ThresholdNotMet { got: usize, need: usize },
    #[error("Handler error: {0}")]
    Handler(#[from] crate::handlers::AuraHandlerError),
}

/// Message types for ceremony choreography

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThresholdSignRequest {
    pub session_id: SessionId,
    pub message_to_sign: Vec<u8>,
    pub binding_context: Vec<u8>,
    pub threshold: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThresholdNonceCommit {
    pub session_id: SessionId,
    pub signer_id: DeviceId,
    pub commitment: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThresholdChallengeRequest {
    pub session_id: SessionId,
    pub all_commitments: Vec<Vec<u8>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThresholdPartialSig {
    pub session_id: SessionId,
    pub signer_id: DeviceId,
    pub signature: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThresholdAttestedResult {
    pub session_id: SessionId,
    pub attested_op: Option<AttestedOp>,
    pub success: bool,
}

/// Threshold ceremony choreography
///
/// Multi-party protocol: Coordinator orchestrates N signers, broadcasts result to M observers
// TEMPORARILY DISABLED DUE TO MACRO CONFLICTS - needs investigation
/*
choreography! {
    protocol ThresholdCeremony {
        roles: Coordinator, Signer1, Signer2, Signer3, Observer1, Observer2;

        // Phase 1: Coordinator broadcasts sign request to all signers
        Coordinator -> Signer1: ThresholdInitiateSign(ThresholdSignRequest);
        Coordinator -> Signer2: ThresholdInitiateSign(ThresholdSignRequest);
        Coordinator -> Signer3: ThresholdInitiateSign(ThresholdSignRequest);

        // Phase 2: Collect nonce commitments from all signers
        Signer1 -> Coordinator: ThresholdCommitNonce(ThresholdNonceCommit);
        Signer2 -> Coordinator: ThresholdCommitNonce(ThresholdNonceCommit);
        Signer3 -> Coordinator: ThresholdCommitNonce(ThresholdNonceCommit);

        // Phase 3: Coordinator broadcasts challenge with all commitments
        Coordinator -> Signer1: ThresholdBroadcastChallenge(ThresholdChallengeRequest);
        Coordinator -> Signer2: ThresholdBroadcastChallenge(ThresholdChallengeRequest);
        Coordinator -> Signer3: ThresholdBroadcastChallenge(ThresholdChallengeRequest);

        // Phase 4: Collect partial signatures from all signers
        Signer1 -> Coordinator: ThresholdSubmitPartial(ThresholdPartialSig);
        Signer2 -> Coordinator: ThresholdSubmitPartial(ThresholdPartialSig);
        Signer3 -> Coordinator: ThresholdSubmitPartial(ThresholdPartialSig);

        // Phase 5: Broadcast final result to signers and observers
        Coordinator -> Signer1: ThresholdBroadcastResult(ThresholdAttestedResult);
        Coordinator -> Signer2: ThresholdBroadcastResult(ThresholdAttestedResult);
        Coordinator -> Signer3: ThresholdBroadcastResult(ThresholdAttestedResult);
        Coordinator -> Observer1: ThresholdBroadcastResult(ThresholdAttestedResult);
        Coordinator -> Observer2: ThresholdBroadcastResult(ThresholdAttestedResult);
    }
}
*/

/// Execute threshold ceremony
pub async fn execute_threshold_ceremony(
    device_id: DeviceId,
    config: CeremonyConfig,
    is_coordinator: bool,
    is_observer: bool,
    signer_index: Option<usize>,
    effect_system: &crate::effects::system::AuraEffectSystem,
) -> Result<CeremonyResult, CeremonyError> {
    // Validate configuration
    let n = config.participants.len();
    let m = config.threshold as usize;

    if m == 0 || m > n {
        return Err(CeremonyError::InvalidConfig(format!(
            "Invalid threshold: {} (must be 1..={})",
            m, n
        )));
    }

    // Create handler adapter
    let mut adapter =
        crate::choreography::AuraHandlerAdapter::new(device_id, effect_system.execution_mode());

    // Execute appropriate role
    if is_coordinator {
        let signers: Vec<DeviceId> = config
            .participants
            .iter()
            .filter(|&&id| id != device_id)
            .copied()
            .collect();

        coordinator_session(&mut adapter, &signers, &config.observers, &config).await
    } else if is_observer {
        let coordinator_id = config.participants[0]; // First participant is coordinator
        observer_session(&mut adapter, coordinator_id).await
    } else {
        let coordinator_id = config.participants[0];
        let signer_idx = signer_index
            .ok_or_else(|| CeremonyError::InvalidConfig("Signer must have index".to_string()))?;

        signer_session(&mut adapter, coordinator_id, device_id, signer_idx, &config).await
    }
}

/// Coordinator's role in threshold ceremony
async fn coordinator_session(
    adapter: &mut crate::choreography::AuraHandlerAdapter,
    signers: &[DeviceId],
    observers: &[DeviceId],
    config: &CeremonyConfig,
) -> Result<CeremonyResult, CeremonyError> {
    let session_id = SessionId::new();

    // Phase 1: Broadcast sign request to all signers
    let op_bytes = bincode::serialize(&config.operation).map_err(|e| {
        CeremonyError::CeremonyFailed(format!("Failed to serialize operation: {}", e))
    })?;

    let binding_msg = config.parent_binding.binding_message(&op_bytes);

    let sign_request = ThresholdSignRequest {
        session_id: session_id.clone(),
        message_to_sign: binding_msg.clone(),
        binding_context: op_bytes.clone(),
        threshold: config.threshold,
    };

    for signer_id in signers {
        adapter
            .send(*signer_id, sign_request.clone())
            .await
            .map_err(|e| {
                CeremonyError::Communication(format!("Failed to send sign request: {}", e))
            })?;
    }

    // Phase 2: Collect nonce commitments from all signers
    let mut nonce_commitments = HashMap::new();
    let mut commitment_list = Vec::new();

    for signer_id in signers {
        let commit: ThresholdNonceCommit = adapter.recv_from(*signer_id).await.map_err(|e| {
            CeremonyError::Communication(format!("Failed to receive commitment: {}", e))
        })?;

        if commit.session_id != session_id {
            return Err(CeremonyError::CeremonyFailed(
                "Session ID mismatch in commitment".to_string(),
            ));
        }

        commitment_list.push(commit.commitment.clone());
        nonce_commitments.insert(commit.signer_id, commit);
    }

    // Phase 3: Broadcast challenge with all commitments
    let challenge_request = ThresholdChallengeRequest {
        session_id: session_id.clone(),
        all_commitments: commitment_list,
    };

    for signer_id in signers {
        adapter
            .send(*signer_id, challenge_request.clone())
            .await
            .map_err(|e| {
                CeremonyError::Communication(format!("Failed to send challenge: {}", e))
            })?;
    }

    // Phase 4: Collect partial signatures
    let mut partial_signatures = HashMap::new();

    for signer_id in signers {
        let partial: ThresholdPartialSig = adapter.recv_from(*signer_id).await.map_err(|e| {
            CeremonyError::Communication(format!("Failed to receive partial signature: {}", e))
        })?;

        if partial.session_id != session_id {
            return Err(CeremonyError::CeremonyFailed(
                "Session ID mismatch in signature".to_string(),
            ));
        }

        partial_signatures.insert(partial.signer_id, partial);
    }

    // Check threshold
    if partial_signatures.len() < config.threshold as usize {
        return Err(CeremonyError::ThresholdNotMet {
            got: partial_signatures.len(),
            need: config.threshold as usize,
        });
    }

    // Phase 5: Aggregate signatures (TODO fix - Simplified - real implementation would use FROST)
    let aggregated_signature = aggregate_partial_signatures(&partial_signatures, adapter).await?;

    let attested_op = AttestedOp {
        op: config.operation.clone(),
        agg_sig: aggregated_signature,
        signer_count: partial_signatures.len() as u16,
    };

    let result = ThresholdAttestedResult {
        session_id: session_id.clone(),
        attested_op: Some(attested_op.clone()),
        success: true,
    };

    // Broadcast result to all signers
    for signer_id in signers {
        adapter
            .send(*signer_id, result.clone())
            .await
            .map_err(|e| {
                CeremonyError::Communication(format!("Failed to send result to signer: {}", e))
            })?;
    }

    // Broadcast result to all observers
    for observer_id in observers {
        adapter
            .send(*observer_id, result.clone())
            .await
            .map_err(|e| {
                CeremonyError::Communication(format!("Failed to send result to observer: {}", e))
            })?;
    }

    Ok(CeremonyResult {
        attested_op: Some(attested_op),
        signatures_collected: partial_signatures.len() as u16,
        success: true,
    })
}

/// Signer's role in threshold ceremony
async fn signer_session(
    adapter: &mut crate::choreography::AuraHandlerAdapter,
    coordinator_id: DeviceId,
    signer_id: DeviceId,
    _signer_index: usize,
    _config: &CeremonyConfig,
) -> Result<CeremonyResult, CeremonyError> {
    // Phase 1: Receive sign request
    let sign_request: ThresholdSignRequest =
        adapter.recv_from(coordinator_id).await.map_err(|e| {
            CeremonyError::Communication(format!("Failed to receive sign request: {}", e))
        })?;

    // Phase 2: Generate and send nonce commitment
    let nonce = adapter.effects().random_bytes(32).await;
    let commitment = adapter.effects().hash(&nonce).await;

    let nonce_commit = ThresholdNonceCommit {
        session_id: sign_request.session_id.clone(),
        signer_id,
        commitment: commitment.to_vec(),
    };

    adapter
        .send(coordinator_id, nonce_commit)
        .await
        .map_err(|e| CeremonyError::Communication(format!("Failed to send commitment: {}", e)))?;

    // Phase 3: Receive challenge with all commitments
    let challenge: ThresholdChallengeRequest = adapter
        .recv_from(coordinator_id)
        .await
        .map_err(|e| CeremonyError::Communication(format!("Failed to receive challenge: {}", e)))?;

    if challenge.session_id != sign_request.session_id {
        return Err(CeremonyError::CeremonyFailed(
            "Session ID mismatch".to_string(),
        ));
    }

    // Phase 4: Generate and send partial signature
    // TODO fix - Simplified: hash message with nonce
    let mut sig_input = Vec::new();
    sig_input.extend_from_slice(&sign_request.message_to_sign);
    sig_input.extend_from_slice(&nonce);
    for comm in &challenge.all_commitments {
        sig_input.extend_from_slice(comm);
    }

    let signature = adapter.effects().hash(&sig_input).await;

    let partial_sig = ThresholdPartialSig {
        session_id: sign_request.session_id.clone(),
        signer_id,
        signature: signature.to_vec(),
    };

    adapter
        .send(coordinator_id, partial_sig)
        .await
        .map_err(|e| {
            CeremonyError::Communication(format!("Failed to send partial signature: {}", e))
        })?;

    // Phase 5: Receive final result
    let result: ThresholdAttestedResult = adapter
        .recv_from(coordinator_id)
        .await
        .map_err(|e| CeremonyError::Communication(format!("Failed to receive result: {}", e)))?;

    Ok(CeremonyResult {
        attested_op: result.attested_op,
        signatures_collected: 1,
        success: result.success,
    })
}

/// Observer's role in threshold ceremony
async fn observer_session(
    adapter: &mut crate::choreography::AuraHandlerAdapter,
    coordinator_id: DeviceId,
) -> Result<CeremonyResult, CeremonyError> {
    // Phase 5: Observers only receive the final result
    let result: ThresholdAttestedResult = adapter
        .recv_from(coordinator_id)
        .await
        .map_err(|e| CeremonyError::Communication(format!("Failed to receive result: {}", e)))?;

    Ok(CeremonyResult {
        attested_op: result.attested_op,
        signatures_collected: 0, // Observers don't contribute signatures
        success: result.success,
    })
}

/// Aggregate partial signatures (TODO fix - Simplified)
async fn aggregate_partial_signatures(
    partial_signatures: &HashMap<DeviceId, ThresholdPartialSig>,
    adapter: &mut crate::choreography::AuraHandlerAdapter,
) -> Result<Vec<u8>, CeremonyError> {
    // TODO fix - Simplified aggregation: hash all partial signatures together
    // Real implementation would use FROST aggregation
    let mut combined = Vec::new();
    for (_, partial) in partial_signatures {
        combined.extend_from_slice(&partial.signature);
    }

    let aggregated = adapter.effects().hash(&combined).await;
    Ok(aggregated.to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::effects::system::AuraEffectSystem;
    use aura_core::tree::{LeafId, LeafNode, LeafRole, NodeIndex, TreeOpKind};

    fn create_test_config() -> CeremonyConfig {
        CeremonyConfig {
            participants: vec![DeviceId::new(), DeviceId::new(), DeviceId::new()],
            observers: vec![],
            threshold: 2,
            operation: TreeOp {
                parent_commitment: [1u8; 32],
                parent_epoch: 1,
                op: TreeOpKind::AddLeaf {
                    leaf: LeafNode {
                        leaf_id: LeafId(1),
                        role: LeafRole::Device,
                        public_key: vec![1, 2, 3],
                        meta: vec![],
                    },
                    under: NodeIndex(0),
                },
            },
            parent_binding: ParentBinding::new(1, [1u8; 32], [2u8; 32]),
        }
    }

    #[test]
    fn test_parent_binding_message() {
        let binding = ParentBinding::new(5, [1u8; 32], [2u8; 32]);
        let op_bytes = vec![3, 4, 5];
        let msg = binding.binding_message(&op_bytes);

        assert!(msg.starts_with(b"TREE_OP_SIG"));
        assert!(msg.len() > 11);
    }

    #[test]
    fn test_config_validation() {
        let mut config = create_test_config();
        config.threshold = 0;

        let device_id = DeviceId::new();
        let effect_system = AuraEffectSystem::for_testing(device_id);

        let runtime = tokio::runtime::Runtime::new().unwrap();
        let result = runtime.block_on(execute_threshold_ceremony(
            device_id,
            config,
            true,
            false,
            None,
            &effect_system,
        ));

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            CeremonyError::InvalidConfig(_)
        ));
    }
}
