//! Consensus protocol coordination and execution
//!
//! This module unifies the coordinator and choreography logic for running
//! the Aura Consensus protocol. It manages consensus instances, orchestrates
//! the message flow, and integrates with the FROST cryptography layer.

use super::{
    core::{self, ConsensusPhase as CorePhase, ConsensusState as CoreState},
    dkg::{self, DealerPackage, DkgConfig, DkgTranscriptStore},
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
    AuraError, AuthorityId, ContextId, Hash32, Prestate, Result,
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
    /// Pure core state for invariant validation
    /// Quint: protocol_consensus.qnt / Lean: Aura.Consensus.Types
    core_state: CoreState,
}

impl ProtocolInstance {
    /// Convert effectful phase to pure core phase
    /// Quint: ConsensusPhase / Lean: Aura.Consensus.Types.ConsensusPhase
    fn to_core_phase(&self) -> CorePhase {
        match self.phase {
            ConsensusPhase::Execute => CorePhase::FastPathActive,
            ConsensusPhase::NonceCommit => CorePhase::FastPathActive,
            ConsensusPhase::Sign => CorePhase::FastPathActive,
            ConsensusPhase::Result => CorePhase::Committed,
        }
    }

    /// Synchronize pure core state with effectful state
    fn sync_core_state(&mut self) {
        self.core_state.phase = self.to_core_phase();
        // Sync proposals from tracker
        self.core_state.proposals = self
            .tracker
            .get_signatures()
            .iter()
            .map(|sig| core::ShareProposal {
                witness: format!("{}", sig.signer),
                result_id: format!("{:?}", self.operation_hash),
                share: core::ShareData {
                    share_value: hex::encode(&sig.signature),
                    nonce_binding: String::new(),
                    data_binding: format!("{:?}", self.prestate_hash),
                },
            })
            .collect();
    }

    /// Check invariants after state transitions (debug mode only)
    fn assert_invariants(&self) {
        debug_assert!(
            core::check_invariants(&self.core_state).is_ok(),
            "Consensus invariant violation: {:?}",
            core::check_invariants(&self.core_state).err()
        );
    }
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
    /// Evict stale protocol instances that have exceeded the configured timeout.
    pub async fn cleanup_stale_instances(&self, now_ms: u64) -> usize {
        let timeout_ms = self.config.timeout_ms;
        let mut removed = 0usize;
        let mut instances = self.instances.write().await;
        instances.retain(|_, instance| {
            let stale = now_ms.saturating_sub(instance.start_time_ms) > timeout_ms;
            if stale {
                removed += 1;
            }
            !stale
        });
        removed
    }

    /// Create a new consensus protocol instance
    pub fn new(
        authority_id: AuthorityId,
        config: ConsensusConfig,
        key_packages: HashMap<AuthorityId, Share>,
        group_public_key: PublicKeyPackage,
    ) -> Result<Self> {
        let frost_orchestrator = FrostConsensusOrchestrator::new(
            config.clone(),
            key_packages,
            group_public_key.clone(),
        )?;

        Ok(Self {
            authority_id,
            config,
            frost_orchestrator,
            group_public_key,
            instances: Arc::new(RwLock::new(HashMap::new())),
        })
    }

    /// Run consensus as coordinator
    pub async fn run_consensus<T: serde::Serialize>(
        &self,
        prestate: &Prestate,
        operation: &T,
        random: &(impl RandomEffects + ?Sized),
        time: &(impl PhysicalTimeEffects + ?Sized),
    ) -> Result<ConsensusResponse> {
        // Best-effort cleanup of stale instances before starting a new run.
        if let Ok(now) = time.physical_time().await {
            let _ = self.cleanup_stale_instances(now.ts_ms).await;
        }
        // Serialize operation
        let operation_bytes =
            serde_json::to_vec(operation).map_err(|e| AuraError::serialization(e.to_string()))?;

        // Compute hashes
        let prestate_hash = prestate.compute_hash();
        let operation_hash = crate::hash_operation(&operation_bytes)?;

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

    /// Finalize a DKG transcript and persist its commit reference.
    pub async fn finalize_dkg_transcript<S: DkgTranscriptStore + ?Sized>(
        &self,
        context: ContextId,
        config: &DkgConfig,
        packages: Vec<DealerPackage>,
        store: &S,
    ) -> Result<aura_journal::fact::DkgTranscriptCommit> {
        let transcript = dkg::ceremony::run_dkg_ceremony(config, packages)?;
        dkg::ceremony::persist_transcript(store, context, &transcript).await
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
        // Best-effort cleanup of stale instances before handling messages.
        if let Ok(now) = time.physical_time().await {
            let _ = self.cleanup_stale_instances(now.ts_ms).await;
        }
        match message {
            ConsensusMessage::Execute {
                consensus_id,
                prestate_hash,
                operation_hash,
                operation_bytes,
                cached_commitments,
            } => {
                // Initialize pure core state for invariant validation
                // Quint: startConsensus action / Lean: Consensus.Agreement
                let core_state = CoreState {
                    cid: format!("{consensus_id}"),
                    operation: String::new(), // Set from operation_bytes if needed
                    prestate_hash: format!("{prestate_hash:?}"),
                    threshold: self.config.threshold as usize,
                    witnesses: self
                        .config
                        .witness_set
                        .iter()
                        .map(|w| format!("{w}"))
                        .collect(),
                    initiator: format!("{coordinator}"),
                    phase: CorePhase::FastPathActive,
                    proposals: Vec::new(),
                    commit_fact: None,
                    fallback_timer_active: false,
                    equivocators: std::collections::BTreeSet::new(),
                };

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
                    start_time_ms: time
                        .physical_time()
                        .await
                        .map_err(|e| AuraError::internal(format!("time error: {e}")))?
                        .ts_ms,
                    nonce_token: None,
                    core_state,
                };

                // Verify invariants on initialization
                instance.assert_invariants();

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
                    AuraError::internal(format!("CommitFact verification failed: {e}"))
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
        .map_err(|e| AuraError::crypto(format!("Invalid signing share: {e}")))?;

        let nonces = frost_ed25519::round1::SigningNonces::new(&signing_share, &mut rng);
        let commitment = NonceCommitment {
            signer: share.identifier,
            commitment: nonces
                .commitments()
                .serialize()
                .map_err(|e| AuraError::crypto(format!("Failed to serialize commitments: {e}")))?,
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
            .map_err(|e| AuraError::crypto(format!("Invalid signing share: {e}")))?;
            let nonces = frost_ed25519::round1::SigningNonces::new(&signing_share, &mut rng);
            let commitment = NonceCommitment {
                signer: share.identifier,
                commitment: nonces.commitments().serialize().map_err(|e| {
                    AuraError::crypto(format!("Failed to serialize commitments: {e}"))
                })?,
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
                    instance.sync_core_state();
                    instance.assert_invariants();
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

                // Sync core state after adding share
                // Quint: applyShare action / Lean: Consensus.Agreement
                instance.sync_core_state();
                instance.assert_invariants();

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
                AuraError::crypto(format!("Invalid group public key package: {e}"))
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
        .map_err(|e| AuraError::crypto(format!("FROST aggregation failed: {e}")))?;

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
    let config = ConsensusConfig::new(params.threshold, params.witnesses, params.epoch)?;
    // Derive coordinator ID deterministically from the prestate hash to keep coordination scoped to the instance.
    let prestate_hash = prestate.compute_hash();
    let mut entropy = [0u8; 32];
    entropy.copy_from_slice(&prestate_hash.0);
    let authority_id = AuthorityId::new_from_entropy(entropy);

    let protocol = ConsensusProtocol::new(
        authority_id,
        config,
        params.key_packages,
        params.group_public_key,
    )?;

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
        let witnesses = vec![
            AuthorityId::new_from_entropy([1u8; 32]),
            AuthorityId::new_from_entropy([2u8; 32]),
        ];
        let config = ConsensusConfig::new(2, witnesses, Epoch::from(1)).unwrap();
        let authority_id = AuthorityId::new_from_entropy([3u8; 32]);

        let protocol = ConsensusProtocol::new(
            authority_id,
            config,
            HashMap::new(),
            PublicKeyPackage::new(vec![0u8; 32], std::collections::BTreeMap::new(), 1, 1),
        )
        .unwrap();

        // Protocol should be created successfully
        let _ = protocol;
    }
}
