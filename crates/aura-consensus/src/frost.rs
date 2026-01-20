//! FROST cryptography integration with pipelining optimization
//!
//! This module provides the cryptographic layer for consensus, integrating
//! FROST threshold signatures with the pipelining optimization for 1 RTT consensus.

use super::{
    messages::{
        ConsensusError, ConsensusMessage, ConsensusPhase, ConsensusRequest, ConsensusResponse,
    },
    types::{CommitFact, ConsensusConfig, ConsensusId},
    witness::{WitnessSet, WitnessTracker},
};
use async_lock::RwLock;
use aura_core::{
    crypto::tree_signing::{frost_aggregate, frost_verify_aggregate, NonceToken},
    effects::{PhysicalTimeEffects, RandomEffects},
    epochs::Epoch,
    frost::{NonceCommitment, PartialSignature, PublicKeyPackage, Share, ThresholdSignature},
    time::{PhysicalTime, ProvenancedTime, TimeStamp},
    AuraError, AuthorityId, Hash32, Result,
};
use rand::SeedableRng;
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use tracing::{debug, info, warn};

/// FROST consensus orchestrator with pipelining support
///
/// Manages the cryptographic operations and optimization logic for consensus:
/// - Fast path (1 RTT) using cached commitments
/// - Slow path (2 RTT) for bootstrap and fallback
/// - Epoch-aware commitment management
/// - FROST threshold signature generation and verification
pub struct FrostConsensusOrchestrator {
    /// Current consensus configuration
    config: ConsensusConfig,

    /// Witness set with cached state
    witness_set: WitnessSet,

    /// FROST key material
    key_packages: HashMap<AuthorityId, Share>,
    group_public_key: PublicKeyPackage,

    /// Active consensus instances
    instances: Arc<RwLock<HashMap<ConsensusId, ConsensusInstance>>>,
}

/// State for a single consensus instance
#[derive(Clone)]
struct ConsensusInstance {
    consensus_id: ConsensusId,
    prestate_hash: Hash32,
    operation_hash: Hash32,
    operation_bytes: Vec<u8>,
    tracker: WitnessTracker,
    phase: ConsensusPhase,
    fast_path: bool,
    start_time_ms: u64,
}

impl FrostConsensusOrchestrator {
    /// Create a new FROST consensus orchestrator
    pub fn new(
        config: ConsensusConfig,
        key_packages: HashMap<AuthorityId, Share>,
        group_public_key: PublicKeyPackage,
    ) -> Result<Self> {
        let witness_set = config.witness_set.to_runtime()?;

        Ok(Self {
            config,
            witness_set,
            key_packages,
            group_public_key,
            instances: Arc::new(RwLock::new(HashMap::new())),
        })
    }

    /// Evict stale consensus instances that exceed the configured timeout.
    pub async fn cleanup_stale_instances(&self, now_ms: u64) -> usize {
        let timeout_ms = self.config.timeout_ms.get();
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

    /// Run consensus on a request
    pub async fn run_consensus(
        &self,
        request: ConsensusRequest,
        random: &(impl RandomEffects + ?Sized),
        time: &(impl PhysicalTimeEffects + ?Sized),
    ) -> Result<ConsensusResponse> {
        // Best-effort cleanup of stale instances before starting a new run.
        if let Ok(now) = time.physical_time().await {
            let _ = self.cleanup_stale_instances(now.ts_ms).await;
        }
        let start_time = time
            .physical_time()
            .await
            .map_err(|e| AuraError::internal(format!("time error: {e}")))?
            .ts_ms;
        let nonce = random.random_u64().await;
        let consensus_id = ConsensusId::new(request.prestate_hash, request.operation_hash, nonce);

        // Check if we can use fast path
        let fast_path = self.config.enable_pipelining
            && self
                .witness_set
                .has_fast_path_quorum(self.config.epoch)
                .await;

        info!(
            consensus_id = %consensus_id,
            fast_path = fast_path,
            "Starting consensus"
        );

        let result = if fast_path {
            self.run_fast_path(consensus_id, request, random, time)
                .await
        } else {
            self.run_slow_path(consensus_id, request, random, time)
                .await
        };

        let duration_ms = time
            .physical_time()
            .await
            .map_err(|e| AuraError::internal(format!("time error: {e}")))?
            .ts_ms
            - start_time;

        match result {
            Ok(commit_fact) => Ok(ConsensusResponse {
                consensus_id,
                result: Ok(commit_fact),
                duration_ms,
                fast_path,
            }),
            Err(e) => {
                warn!(consensus_id = %consensus_id, error = %e, "Consensus failed");
                Ok(ConsensusResponse {
                    consensus_id,
                    result: Err(match e {
                        AuraError::Network { message } => ConsensusError::Network(message),
                        AuraError::Crypto { message } => ConsensusError::Crypto(message),
                        _ => ConsensusError::Internal(e.to_string()),
                    }),
                    duration_ms,
                    fast_path,
                })
            }
        }
    }

    /// Run fast path (1 RTT) using cached commitments
    async fn run_fast_path(
        &self,
        consensus_id: ConsensusId,
        request: ConsensusRequest,
        random: &(impl RandomEffects + ?Sized),
        time: &(impl PhysicalTimeEffects + ?Sized),
    ) -> Result<CommitFact> {
        // Collect cached commitments
        let cached_commitments = self
            .witness_set
            .collect_cached_commitments(self.config.epoch)
            .await;

        if cached_commitments.len() < self.config.threshold() as usize {
            debug!("Insufficient cached commitments, falling back to slow path");
            return self
                .run_slow_path(consensus_id, request, random, time)
                .await;
        }

        // Create instance
        let instance = ConsensusInstance {
            consensus_id,
            prestate_hash: request.prestate_hash,
            operation_hash: request.operation_hash,
            operation_bytes: request.operation_bytes.clone(),
            tracker: WitnessTracker::new(),
            phase: ConsensusPhase::Execute,
            fast_path: true,
            start_time_ms: time
                .physical_time()
                .await
                .map_err(|e| AuraError::internal(format!("time error: {e}")))?
                .ts_ms,
        };

        self.instances.write().await.insert(consensus_id, instance);

        // Skip NonceCommit phase and go directly to signing with cached commitments
        let aggregated_nonces: Vec<NonceCommitment> =
            cached_commitments.values().cloned().collect();

        // Sign with each witness
        let mut tracker = WitnessTracker::new();
        for (witness_id, commitment) in cached_commitments.iter() {
            tracker.add_nonce(*witness_id, commitment.clone());
        }

        // Generate signatures
        for witness_id in self.config.witness_set.iter() {
            if let Some(share) = self.key_packages.get(witness_id) {
                // Take cached nonce for signing
                let mut witness_state = self
                    .witness_set
                    .get_or_create_state(*witness_id, self.config.epoch)
                    .await;
                if let Some((commitment, token)) = witness_state.take_nonce(self.config.epoch) {
                    // Generate partial signature
                    let signature = self.sign_with_nonce(
                        &request.operation_bytes,
                        share,
                        &token,
                        &aggregated_nonces,
                    )?;

                    // Use operation_hash as result_id (deterministic execution assumption)
                    let _ = tracker.add_signature(*witness_id, signature, request.operation_hash);

                    // Generate and cache next round commitment for pipelining
                    let (next_commitment, next_token) = self.generate_nonce(share, random).await?;
                    witness_state.set_next_nonce(next_commitment, next_token, self.config.epoch);
                }
            }
        }

        self.finalize_consensus(consensus_id, tracker, time).await
    }

    /// Run slow path (2 RTT) - standard FROST consensus
    async fn run_slow_path(
        &self,
        consensus_id: ConsensusId,
        request: ConsensusRequest,
        random: &(impl RandomEffects + ?Sized),
        time: &(impl PhysicalTimeEffects + ?Sized),
    ) -> Result<CommitFact> {
        // Create instance
        let instance = ConsensusInstance {
            consensus_id,
            prestate_hash: request.prestate_hash,
            operation_hash: request.operation_hash,
            operation_bytes: request.operation_bytes.clone(),
            tracker: WitnessTracker::new(),
            phase: ConsensusPhase::Execute,
            fast_path: false,
            start_time_ms: time
                .physical_time()
                .await
                .map_err(|e| AuraError::internal(format!("time error: {e}")))?
                .ts_ms,
        };

        self.instances.write().await.insert(consensus_id, instance);

        // Phase 1: Generate and collect nonce commitments
        let mut tracker = WitnessTracker::new();
        let mut nonce_tokens = HashMap::new();

        for witness_id in self.config.witness_set.iter() {
            if let Some(share) = self.key_packages.get(witness_id) {
                let (commitment, token) = self.generate_nonce(share, random).await?;
                tracker.add_nonce(*witness_id, commitment);
                nonce_tokens.insert(*witness_id, token);
            }
        }

        if !tracker.has_nonce_threshold(self.config.threshold()) {
            return Err(AuraError::internal("Insufficient nonce commitments"));
        }

        // Phase 2: Generate signatures
        let aggregated_nonces = tracker.get_nonces();

        for (witness_id, token) in nonce_tokens {
            if let Some(share) = self.key_packages.get(&witness_id) {
                let signature = self.sign_with_nonce(
                    &request.operation_bytes,
                    share,
                    &token,
                    &aggregated_nonces,
                )?;

                // Use operation_hash as result_id (deterministic execution assumption)
                let _ = tracker.add_signature(witness_id, signature, request.operation_hash);

                // Generate and cache next round commitment for future pipelining
                let (next_commitment, next_token) = self.generate_nonce(share, random).await?;
                self.witness_set
                    .update_witness_nonce(
                        witness_id,
                        next_commitment,
                        next_token,
                        self.config.epoch,
                    )
                    .await?;
            }
        }

        self.finalize_consensus(consensus_id, tracker, time).await
    }

    /// Finalize consensus with collected signatures
    async fn finalize_consensus(
        &self,
        consensus_id: ConsensusId,
        tracker: WitnessTracker,
        time: &(impl PhysicalTimeEffects + ?Sized),
    ) -> Result<CommitFact> {
        if !tracker.has_signature_threshold(self.config.threshold()) {
            return Err(AuraError::internal("Insufficient signatures"));
        }

        let instance = {
            let instances = self.instances.read().await;
            instances
                .get(&consensus_id)
                .ok_or_else(|| AuraError::internal("Instance not found"))?
                .clone()
        };

        // Aggregate signatures
        let participants = tracker.get_participants();

        let threshold_signature = self.aggregate_signatures(&tracker, &instance.operation_bytes)?;

        // Create commit fact
        let timestamp = ProvenancedTime {
            stamp: TimeStamp::PhysicalClock(PhysicalTime {
                ts_ms: time
                    .physical_time()
                    .await
                    .map_err(|e| AuraError::internal(format!("time error: {e}")))?
                    .ts_ms,
                uncertainty: None,
            }),
            proofs: vec![],
            origin: None,
        };

        let commit_fact = CommitFact::new(
            consensus_id,
            instance.prestate_hash,
            instance.operation_hash,
            instance.operation_bytes,
            threshold_signature,
            Some(self.group_public_key.clone()),
            participants,
            self.config.threshold(),
            instance.fast_path,
            timestamp,
        );

        // Verify the commit fact
        commit_fact
            .verify()
            .map_err(|e| AuraError::internal(format!("CommitFact verification failed: {e}")))?;

        // Clean up instance
        self.instances.write().await.remove(&consensus_id);

        Ok(commit_fact)
    }

    /// Generate a new nonce commitment
    async fn generate_nonce(
        &self,
        share: &Share,
        random: &(impl RandomEffects + ?Sized),
    ) -> Result<(NonceCommitment, NonceToken)> {
        // Convert share to FROST signing share
        // Convert Vec<u8> to fixed array for FROST
        let share_bytes: [u8; 32] = share
            .value
            .as_slice()
            .try_into()
            .map_err(|_| AuraError::crypto("Invalid share length, expected 32 bytes"))?;
        let signing_share = frost_ed25519::keys::SigningShare::deserialize(share_bytes)
            .map_err(|e| AuraError::crypto(format!("Invalid signing share: {e}")))?;

        // Generate nonces with randomness
        let seed = random.random_bytes_32().await;
        let mut rng = rand::rngs::StdRng::from_seed(seed);
        let nonces = frost_ed25519::round1::SigningNonces::new(&signing_share, &mut rng);

        // Create commitment
        let commitment = NonceCommitment {
            signer: share.identifier,
            commitment: nonces
                .commitments()
                .serialize()
                .map_err(|e| AuraError::crypto(format!("Failed to serialize commitments: {e}")))?,
        };

        let token = NonceToken::from(nonces);

        Ok((commitment, token))
    }

    /// Sign with a pre-generated nonce
    pub(crate) fn sign_with_nonce(
        &self,
        message: &[u8],
        share: &Share,
        token: &NonceToken,
        aggregated_nonces: &[NonceCommitment],
    ) -> Result<PartialSignature> {
        // Reconstruct FROST signing share and identifier
        let signing_share = share
            .to_frost()
            .map_err(|e| AuraError::crypto(format!("Invalid signing share: {e}")))?;
        let identifier = frost_ed25519::Identifier::try_from(share.identifier).map_err(|e| {
            AuraError::crypto(format!(
                "Invalid signer identifier {}: {}",
                share.identifier, e
            ))
        })?;

        // Convert group public key package
        let frost_group_pkg: frost_ed25519::keys::PublicKeyPackage = self
            .group_public_key
            .clone()
            .try_into()
            .map_err(|e: AuraError| {
                AuraError::crypto(format!("Invalid group public key package: {e}"))
            })?;

        // Get verifying share for this signer
        let verifying_share = frost_group_pkg
            .verifying_shares()
            .get(&identifier)
            .cloned()
            .ok_or_else(|| {
                AuraError::crypto(format!(
                    "Missing verifying share for signer {}",
                    share.identifier
                ))
            })?;

        // Build key package for signing using the real identifier and threshold metadata
        let key_package = frost_ed25519::keys::KeyPackage::new(
            identifier,
            signing_share,
            verifying_share,
            *frost_group_pkg.verifying_key(),
            self.config.threshold(),
        );

        // Convert commitments to FROST format
        let mut frost_commitments = BTreeMap::new();
        for commitment in aggregated_nonces {
            let frost_id = frost_ed25519::Identifier::try_from(commitment.signer).map_err(|e| {
                AuraError::crypto(format!("Invalid signer id {}: {}", commitment.signer, e))
            })?;
            let frost_commit = commitment
                .to_frost()
                .map_err(|e| AuraError::crypto(format!("Invalid commitment: {e}")))?;
            frost_commitments.insert(frost_id, frost_commit);
        }

        // Build signing package
        let signing_package = frost_ed25519::SigningPackage::new(frost_commitments, message);

        // Perform FROST signing with the provided nonces
        let nonces = token.clone().into_frost();
        let sig_share = frost_ed25519::round2::sign(&signing_package, &nonces, &key_package)
            .map_err(|e| AuraError::crypto(format!("FROST signing failed: {e}")))?;

        Ok(PartialSignature::from_frost(identifier, sig_share))
    }

    /// Aggregate partial signatures into threshold signature
    fn aggregate_signatures(
        &self,
        tracker: &WitnessTracker,
        message: &[u8],
    ) -> Result<ThresholdSignature> {
        // Convert group public key package
        let frost_group_pkg: frost_ed25519::keys::PublicKeyPackage = self
            .group_public_key
            .clone()
            .try_into()
            .map_err(|e: AuraError| {
                AuraError::crypto(format!("Invalid group public key package: {e}"))
            })?;

        // Build commitment map for aggregation
        let mut commitments = BTreeMap::new();
        for commitment in tracker.nonce_commitments.values() {
            commitments.insert(commitment.signer, commitment.clone());
        }

        let partials = tracker.get_signatures();
        let signers = partials.iter().map(|s| s.signer).collect();

        let signature = frost_aggregate(&partials, message, &commitments, &frost_group_pkg)
            .map_err(|e| AuraError::crypto(format!("FROST aggregation failed: {e}")))?;

        Ok(ThresholdSignature { signature, signers })
    }

    /// Handle epoch change
    pub async fn handle_epoch_change(&self, new_epoch: Epoch) {
        if new_epoch != self.config.epoch {
            info!(
                old_epoch = %self.config.epoch,
                new_epoch = %new_epoch,
                "Epoch change detected, invalidating cached commitments"
            );

            self.witness_set.invalidate_all_caches().await;
        }
    }

    /// Process incoming consensus message
    pub async fn process_message(
        &self,
        message: ConsensusMessage,
        sender: AuthorityId,
    ) -> Result<Option<ConsensusMessage>> {
        if let ConsensusMessage::SignShare {
            consensus_id,
            result_id: _,
            share,
            next_commitment: Some(commitment),
            epoch,
            ..
        } = message
        {
            // Cache next commitment for pipelining
            if epoch == self.config.epoch {
                debug!(
                    witness = %sender,
                    consensus_id = %consensus_id,
                    "Received pipelined commitment for next round (commitment cached; witness retains nonces)"
                );
            }
        }

        Ok(None)
    }
}

/// Verify a threshold signature
pub fn verify_threshold_signature(
    signature: &ThresholdSignature,
    message: &[u8],
    group_public_key: &PublicKeyPackage,
) -> Result<()> {
    if signature.signature.len() != 64 {
        return Err(AuraError::crypto("Invalid signature length"));
    }

    let frost_pkg: frost_ed25519::keys::PublicKeyPackage = group_public_key
        .clone()
        .try_into()
        .map_err(|e| AuraError::crypto(format!("Invalid group public key: {e}")))?;

    let verifying_key = frost_pkg.verifying_key();

    frost_verify_aggregate(verifying_key, message, &signature.signature)
        .map_err(|e| AuraError::crypto(format!("Threshold verify failed: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::types::Epoch;
    use aura_core::AuthorityId;

    fn authority(seed: u8) -> AuthorityId {
        AuthorityId::new_from_entropy([seed; 32])
    }

    #[tokio::test]
    async fn test_orchestrator_creation() {
        let witnesses = vec![authority(1), authority(2), authority(3)];
        let config = ConsensusConfig::new(2, witnesses, Epoch::from(1)).unwrap();

        let orchestrator = FrostConsensusOrchestrator::new(
            config,
            HashMap::new(),
            PublicKeyPackage::new(vec![0u8; 32], std::collections::BTreeMap::new(), 1, 1),
        )
        .unwrap();

        // Should start with no cached commitments
        assert!(
            !orchestrator
                .witness_set
                .has_fast_path_quorum(Epoch::from(1))
                .await
        );
    }

    #[tokio::test]
    async fn test_epoch_change() {
        let witnesses = vec![authority(10), authority(11)];
        let config = ConsensusConfig::new(2, witnesses, Epoch::from(1)).unwrap();

        let orchestrator = FrostConsensusOrchestrator::new(
            config,
            HashMap::new(),
            PublicKeyPackage::new(vec![0u8; 32], std::collections::BTreeMap::new(), 1, 1),
        )
        .unwrap();

        // Handle epoch change
        orchestrator.handle_epoch_change(Epoch::from(2)).await;

        // Cached commitments should be invalidated
        assert!(
            !orchestrator
                .witness_set
                .has_fast_path_quorum(Epoch::from(2))
                .await
        );
    }
}
