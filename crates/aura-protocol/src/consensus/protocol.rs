//! Consensus protocol coordination and execution
//!
//! This module unifies the coordinator and choreography logic for running
//! the Aura Consensus protocol. It manages consensus instances, orchestrates
//! the message flow, and integrates with the FROST cryptography layer.

use super::{
    frost::FrostConsensusOrchestrator,
    messages::{ConsensusMessage, ConsensusPhase, ConsensusRequest, ConsensusResponse},
    types::{CommitFact, ConsensusConfig, ConsensusId},
    witness::{WitnessSet, WitnessTracker},
};
use async_lock::RwLock;
use aura_core::{
    crypto::tree_signing::frost_aggregate,
    crypto::tree_signing::NonceToken,
    effects::{PhysicalTimeEffects, RandomEffects},
    epochs::Epoch,
    frost::{NonceCommitment, PublicKeyPackage, Share},
    time::{PhysicalTime, ProvenancedTime, TimeStamp},
    AuraError, AuthorityId, Hash32, Prestate, Result,
};
use aura_macros::choreography;
use frost_ed25519;
use rand::SeedableRng;
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
// Timeout support should be implemented via injected timer effects rather than runtime-specific APIs
use tracing::{debug, info, warn};

// Define the consensus choreography protocol
choreography! {
    #[namespace = "aura_consensus"]
    protocol AuraConsensus {
        roles: Coordinator, Witness[n];

        // Phase 1: Initiate consensus
        Coordinator[guard_capability = "initiate_consensus", flow_cost = 100]
        -> Witness[*]: Execute(ConsensusMessage);

        // Phase 2: Collect nonce commitments (slow path only)
        Witness[*][guard_capability = "witness_nonce", flow_cost = 50]
        -> Coordinator: NonceCommit(ConsensusMessage);

        // Phase 3: Request signatures with aggregated nonces (slow path only)
        Coordinator[guard_capability = "aggregate_nonces", flow_cost = 75]
        -> Witness[*]: SignRequest(ConsensusMessage);

        // Phase 4: Collect partial signatures
        Witness[*][guard_capability = "witness_sign", flow_cost = 50, leak = "pipelined_commitment"]
        -> Coordinator: SignShare(ConsensusMessage);

        // Phase 5: Broadcast result
        Coordinator[guard_capability = "finalize_consensus", flow_cost = 100,
                    journal_facts = "consensus_complete"]
        -> Witness[*]: ConsensusResult(ConsensusMessage);
    }
}

/// Protocol coordinator that manages consensus execution
pub struct ConsensusProtocol {
    /// Our authority ID
    authority_id: AuthorityId,

    /// Consensus configuration
    config: ConsensusConfig,

    /// FROST orchestrator for crypto operations
    frost_orchestrator: FrostConsensusOrchestrator,

    /// Group public key package for verification/aggregation
    group_public_key: PublicKeyPackage,

    /// Active protocol instances
    instances: Arc<RwLock<HashMap<ConsensusId, ProtocolInstance>>>,
}

/// State for a single protocol instance
struct ProtocolInstance {
    consensus_id: ConsensusId,
    prestate_hash: Hash32,
    operation_hash: Hash32,
    operation_bytes: Vec<u8>,
    role: ProtocolRole,
    tracker: WitnessTracker,
    phase: ConsensusPhase,
    start_time_ms: u64,
    /// Cached nonce token for signing (slow path)
    nonce_token: Option<NonceToken>,
}

/// Role in the protocol (coordinator or witness)
enum ProtocolRole {
    Coordinator {
        witness_set: WitnessSet,
    },
    Witness {
        coordinator: AuthorityId,
        my_share: Share,
    },
}

impl ConsensusProtocol {
    /// Create a new consensus protocol instance
    pub fn new(
        authority_id: AuthorityId,
        config: ConsensusConfig,
        key_packages: HashMap<AuthorityId, Share>,
        group_public_key: PublicKeyPackage,
    ) -> Self {
        let frost_orchestrator =
            FrostConsensusOrchestrator::new(config.clone(), key_packages, group_public_key.clone());

        Self {
            authority_id,
            config,
            frost_orchestrator,
            group_public_key,
            instances: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Run consensus as coordinator
    pub async fn run_consensus<T: serde::Serialize>(
        &self,
        prestate: &Prestate,
        operation: &T,
        random: &(impl RandomEffects + ?Sized),
        time: &(impl PhysicalTimeEffects + ?Sized),
    ) -> Result<ConsensusResponse> {
        // Serialize operation
        let operation_bytes =
            serde_json::to_vec(operation).map_err(|e| AuraError::serialization(e.to_string()))?;

        // Compute hashes
        let prestate_hash = prestate.compute_hash();
        let operation_hash = crate::consensus::hash_operation(&operation_bytes)?;

        let request = ConsensusRequest {
            prestate_hash,
            operation_bytes,
            operation_hash,
            timeout_ms: Some(self.config.timeout_ms),
        };

        // Use FROST orchestrator for the actual consensus
        self.frost_orchestrator
            .run_consensus(request, random, time)
            .await
    }

    /// Participate as witness in consensus
    pub async fn participate_as_witness(
        &self,
        message: ConsensusMessage,
        coordinator: AuthorityId,
        my_share: Share,
        random: &(impl RandomEffects + ?Sized),
        time: &(impl PhysicalTimeEffects + ?Sized),
    ) -> Result<Option<ConsensusMessage>> {
        match message {
            ConsensusMessage::Execute {
                consensus_id,
                prestate_hash,
                operation_hash,
                operation_bytes,
                cached_commitments,
            } => {
                // Initialize witness instance
                let instance = ProtocolInstance {
                    consensus_id,
                    prestate_hash,
                    operation_hash,
                    operation_bytes: operation_bytes.clone(),
                    role: ProtocolRole::Witness {
                        coordinator,
                        my_share: my_share.clone(),
                    },
                    tracker: WitnessTracker::new(),
                    phase: ConsensusPhase::Execute,
                    start_time_ms: time.physical_time().await.map(|t| t.ts_ms).unwrap_or(0),
                    nonce_token: None,
                };

                self.instances.write().await.insert(consensus_id, instance);

                // Generate nonce commitment (always slow path for correctness)
                self.generate_nonce_commitment(consensus_id, &my_share, random)
                    .await
            }

            ConsensusMessage::SignRequest {
                consensus_id,
                aggregated_nonces,
            } => {
                // Generate signature
                let instances = self.instances.read().await;
                let instance = instances
                    .get(&consensus_id)
                    .ok_or_else(|| AuraError::invalid("Unknown consensus instance"))?;

                self.generate_signature_response(
                    consensus_id,
                    &instance.operation_bytes,
                    aggregated_nonces,
                    &my_share,
                    random,
                )
                .await
            }

            ConsensusMessage::ConsensusResult { commit_fact } => {
                // Verify and store result
                commit_fact.verify().map_err(|e| {
                    AuraError::internal(format!("CommitFact verification failed: {}", e))
                })?;
                self.instances
                    .write()
                    .await
                    .remove(&commit_fact.consensus_id);
                info!(consensus_id = %commit_fact.consensus_id, "Consensus completed");
                Ok(None)
            }

            _ => Ok(None),
        }
    }

    /// Generate nonce commitment (witness role)
    async fn generate_nonce_commitment(
        &self,
        consensus_id: ConsensusId,
        share: &Share,
        random: &(impl RandomEffects + ?Sized),
    ) -> Result<Option<ConsensusMessage>> {
        // Generate FROST nonces and commitment for this witness
        let seed = random.random_bytes_32().await;
        let mut rng = rand::rngs::StdRng::from_seed(seed);

        let signing_share = frost_ed25519::keys::SigningShare::deserialize(
            share
                .value
                .clone()
                .try_into()
                .map_err(|_| AuraError::crypto("Invalid signing share length"))?,
        )
        .map_err(|e| AuraError::crypto(format!("Invalid signing share: {}", e)))?;

        let nonces = frost_ed25519::round1::SigningNonces::new(&signing_share, &mut rng);
        let commitment = NonceCommitment {
            signer: share.identifier,
            commitment: nonces.commitments().hiding().serialize().to_vec(),
        };

        // Cache nonce token for signing when SignRequest arrives
        if let Some(instance) = self.instances.write().await.get_mut(&consensus_id) {
            instance.nonce_token = Some(NonceToken::from(nonces));
        }

        Ok(Some(ConsensusMessage::NonceCommit {
            consensus_id,
            commitment,
        }))
    }

    /// Generate signature response (witness role)
    async fn generate_signature_response(
        &self,
        consensus_id: ConsensusId,
        message: &[u8],
        aggregated_nonces: Vec<NonceCommitment>,
        share: &Share,
        random: &(impl RandomEffects + ?Sized),
    ) -> Result<Option<ConsensusMessage>> {
        // Retrieve cached nonce token (slow path) or generate a fresh one if missing
        let mut instances = self.instances.write().await;
        let instance = instances
            .get_mut(&consensus_id)
            .ok_or_else(|| AuraError::invalid("Unknown consensus instance"))?;

        let nonce_token = if let Some(token) = instance.nonce_token.take() {
            token
        } else {
            // Fallback: generate a fresh nonce and append its commitment
            let seed = random.random_bytes_32().await;
            let mut rng = rand::rngs::StdRng::from_seed(seed);
            let signing_share = frost_ed25519::keys::SigningShare::deserialize(
                share
                    .value
                    .clone()
                    .try_into()
                    .map_err(|_| AuraError::crypto("Invalid signing share length"))?,
            )
            .map_err(|e| AuraError::crypto(format!("Invalid signing share: {}", e)))?;
            let nonces = frost_ed25519::round1::SigningNonces::new(&signing_share, &mut rng);
            let commitment = NonceCommitment {
                signer: share.identifier,
                commitment: nonces.commitments().hiding().serialize().to_vec(),
            };
            instance.tracker.add_nonce(self.authority_id, commitment);
            NonceToken::from(nonces)
        };

        // Sign using FROST with provided aggregated nonces
        let signature = self.frost_orchestrator.sign_with_nonce(
            message,
            share,
            &nonce_token,
            &aggregated_nonces,
        )?;

        // No pipelined commitment until interpreter path supports token handoff
        let next_commitment = None;

        Ok(Some(ConsensusMessage::SignShare {
            consensus_id,
            share: signature,
            next_commitment,
            epoch: self.config.epoch,
        }))
    }

    /// Process incoming message (coordinator role)
    pub async fn process_coordinator_message(
        &self,
        message: ConsensusMessage,
        sender: AuthorityId,
    ) -> Result<Option<ConsensusMessage>> {
        let consensus_id = message.consensus_id();
        let mut instances = self.instances.write().await;

        let instance = instances
            .get_mut(&consensus_id)
            .ok_or_else(|| AuraError::invalid("Unknown consensus instance"))?;

        match message {
            ConsensusMessage::NonceCommit { commitment, .. } => {
                instance.tracker.add_nonce(sender, commitment);

                // Check if we have threshold
                if instance.tracker.has_nonce_threshold(self.config.threshold) {
                    instance.phase = ConsensusPhase::Sign;
                    let nonces = instance.tracker.get_nonces();

                    return Ok(Some(ConsensusMessage::SignRequest {
                        consensus_id,
                        aggregated_nonces: nonces,
                    }));
                }
            }

            ConsensusMessage::SignShare {
                share,
                next_commitment,
                epoch,
                ..
            } => {
                instance.tracker.add_signature(sender, share);

                // Cache next commitment if provided
                if let (Some(commitment), _) = (next_commitment, epoch == self.config.epoch) {
                    debug!(sender = %sender, "Cached pipelined commitment for next round");
                    // Would be handled by witness state manager
                }

                // Check if we have threshold
                if instance
                    .tracker
                    .has_signature_threshold(self.config.threshold)
                {
                    return self.finalize_consensus(consensus_id).await;
                }
            }

            ConsensusMessage::Conflict { conflicts, .. } => {
                instance.tracker.add_conflict(sender, conflicts);
                warn!(consensus_id = %consensus_id, sender = %sender, "Conflict reported");
            }

            _ => {}
        }

        Ok(None)
    }

    /// Finalize consensus and create commit fact
    async fn finalize_consensus(
        &self,
        consensus_id: ConsensusId,
    ) -> Result<Option<ConsensusMessage>> {
        let instances = self.instances.read().await;
        let instance = instances
            .get(&consensus_id)
            .ok_or_else(|| AuraError::internal("Instance not found"))?;

        let signatures = instance.tracker.get_signatures();
        let participants = instance.tracker.get_participants();

        // Aggregate using FROST
        let frost_group_pkg: frost_ed25519::keys::PublicKeyPackage = self
            .group_public_key
            .clone()
            .try_into()
            .map_err(|e: String| {
                AuraError::crypto(format!("Invalid group public key package: {}", e))
            })?;

        let mut commitments = BTreeMap::new();
        for (witness, commitment) in &instance.tracker.nonce_commitments {
            commitments.insert(commitment.signer, commitment.clone());
            debug!(witness = %witness, signer = %commitment.signer, "Using nonce commitment for aggregation");
        }

        let aggregated_sig = frost_aggregate(
            &signatures,
            &instance.operation_bytes,
            &commitments,
            &frost_group_pkg,
        )
        .map_err(|e| AuraError::crypto(format!("FROST aggregation failed: {}", e)))?;

        let threshold_signature = aura_core::frost::ThresholdSignature {
            signature: aggregated_sig,
            signers: signatures.iter().map(|s| s.signer).collect(),
        };

        let timestamp = ProvenancedTime {
            stamp: TimeStamp::PhysicalClock(PhysicalTime {
                ts_ms: 0, // Would be set by time effects
                uncertainty: None,
            }),
            proofs: vec![],
            origin: Some(self.authority_id),
        };

        let commit_fact = CommitFact::new(
            consensus_id,
            instance.prestate_hash,
            instance.operation_hash,
            instance.operation_bytes.clone(),
            threshold_signature,
            None, // Would include group public key
            participants,
            self.config.threshold,
            instance.phase == ConsensusPhase::Execute, // Fast path if we skipped nonce phase
            timestamp,
        );

        Ok(Some(ConsensusMessage::ConsensusResult { commit_fact }))
    }

    /// Handle epoch change
    pub async fn handle_epoch_change(&self, new_epoch: Epoch) {
        self.frost_orchestrator.handle_epoch_change(new_epoch).await;
    }

    /// Get protocol statistics
    pub async fn get_stats(&self) -> ProtocolStats {
        let instances = self.instances.read().await;

        ProtocolStats {
            active_instances: instances.len(),
            epoch: self.config.epoch,
            threshold: self.config.threshold,
            witness_count: self.config.witness_set.len(),
        }
    }
}

/// Protocol statistics
#[derive(Debug, Clone)]
pub struct ProtocolStats {
    pub active_instances: usize,
    pub epoch: Epoch,
    pub threshold: u16,
    pub witness_count: usize,
}

/// Run consensus with default configuration
/// Parameters for consensus execution
pub struct ConsensusParams {
    pub witnesses: Vec<AuthorityId>,
    pub threshold: u16,
    pub key_packages: HashMap<AuthorityId, Share>,
    pub group_public_key: PublicKeyPackage,
    pub epoch: Epoch,
}

pub async fn run_consensus<T: serde::Serialize>(
    prestate: &Prestate,
    operation: &T,
    params: ConsensusParams,
    random: &(impl RandomEffects + ?Sized),
    time: &(impl PhysicalTimeEffects + ?Sized),
) -> Result<CommitFact> {
    let config = ConsensusConfig::new(params.threshold, params.witnesses, params.epoch);
    let authority_id = AuthorityId::new(); // Would be the actual coordinator ID

    let protocol = ConsensusProtocol::new(
        authority_id,
        config,
        params.key_packages,
        params.group_public_key,
    );

    let response = protocol
        .run_consensus(prestate, operation, random, time)
        .await?;

    match response.result {
        Ok(commit_fact) => Ok(commit_fact),
        Err(e) => Err(AuraError::internal(e.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_protocol_creation() {
        let witnesses = vec![AuthorityId::new(), AuthorityId::new()];
        let config = ConsensusConfig::new(2, witnesses, Epoch::from(1));
        let authority_id = AuthorityId::new();

        let protocol = ConsensusProtocol::new(
            authority_id,
            config,
            HashMap::new(),
            PublicKeyPackage::new(vec![0u8; 32], std::collections::BTreeMap::new(), 1, 1),
        );

        // Protocol should be created successfully
        let _ = protocol;
    }
}
