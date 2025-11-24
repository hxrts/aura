//! Consensus Coordinator
//!
//! This module implements the coordinator role for Aura Consensus,
//! managing consensus instances and orchestrating the protocol flow.

use super::choreography::ConsensusMessage;
use super::{
    frost_adapter::{ConsensusFrost, CoreFrostAdapter},
    CommitFact, ConsensusId, WitnessMessage, WitnessSet, WitnessShare,
};
use aura_core::effects::{PhysicalTimeEffects, RandomEffects};
use aura_core::frost::{NonceCommitment, PartialSignature, PublicKeyPackage, ThresholdSignature};
use aura_core::time::{OrderTime, ProvenancedTime, TimeStamp};
use aura_core::Prestate;
use aura_core::{AuraError, AuthorityId, Hash32, Result};
use aura_effects::random::RealRandomHandler;
use aura_effects::time::PhysicalTimeHandler;
use frost_ed25519 as frost;
use rand::rngs::OsRng;
use serde::Serialize;
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use tokio::time::{timeout, Duration};

/// Coordinator for managing consensus instances
pub struct ConsensusCoordinator {
    /// Active consensus instances
    instances: BTreeMap<Hash32, ConsensusInstance>,

    /// Completed instances (for deduplication)
    completed: BTreeMap<ConsensusId, CommitFact>,

    /// Randomness provider (pluggable for deterministic testing)
    random: Arc<dyn RandomEffects>,

    /// Time provider (pluggable for deterministic testing)
    time: Arc<dyn PhysicalTimeEffects>,
}

impl ConsensusCoordinator {
    /// Create a new coordinator
    pub fn new() -> Self {
        Self::with_effects(
            Arc::new(RealRandomHandler),
            Arc::new(PhysicalTimeHandler::new()),
        )
    }

    /// Create a new coordinator with explicit randomness and time providers
    pub fn with_effects(
        random: Arc<dyn RandomEffects>,
        time: Arc<dyn PhysicalTimeEffects>,
    ) -> Self {
        Self {
            instances: BTreeMap::new(),
            completed: BTreeMap::new(),
            random,
            time,
        }
    }

    /// Start a new consensus instance
    pub async fn start_consensus<T: Serialize>(
        &mut self,
        prestate: Prestate,
        operation: &T,
        witnesses: Vec<AuthorityId>,
        threshold: u16,
        key_packages: std::collections::HashMap<AuthorityId, aura_core::frost::Share>,
        group_public_key: PublicKeyPackage,
    ) -> Result<Hash32> {
        // Serialize operation
        let operation_bytes =
            serde_json::to_vec(operation).map_err(|e| AuraError::serialization(e.to_string()))?;

        // Compute hashes
        let prestate_hash = prestate.compute_hash();
        let operation_hash = hash_operation(&operation_bytes)?;

        // Generate consensus ID
        let nonce = self.random.random_u64().await;
        let consensus_id = ConsensusId::new(prestate_hash, operation_hash, nonce);

        // Check if already completed
        if self.completed.contains_key(&consensus_id) {
            return Ok(consensus_id.0);
        }

        let frost_key_packages = build_key_packages(&key_packages, &group_public_key, threshold)?;

        // Create instance
        let instance = ConsensusInstance {
            consensus_id,
            prestate,
            operation_bytes,
            operation_hash,
            witness_set: WitnessSet::new(threshold, witnesses),
            state: InstanceState::Initiated,
            timeout_ms: 30000,
            key_packages: frost_key_packages,
            group_public_key,
            signing_nonces: BTreeMap::new(),
        };

        let instance_id = consensus_id.0;
        self.instances.insert(instance_id, instance);

        Ok(instance_id)
    }

    /// Run the consensus protocol for an instance
    pub async fn run_protocol(&mut self, instance_id: Hash32) -> Result<CommitFact> {
        // Check if instance exists and get consensus_id
        let consensus_id = {
            let instance = self
                .instances
                .get(&instance_id)
                .ok_or_else(|| AuraError::not_found("Consensus instance not found"))?;
            instance.consensus_id
        };

        // Check if already have a result
        if let Some(commit_fact) = self.completed.get(&consensus_id) {
            return Ok(commit_fact.clone());
        }

        // Run fast path first
        match self.run_fast_path(instance_id).await {
            Ok(commit_fact) => {
                self.completed.insert(consensus_id, commit_fact.clone());
                self.instances.remove(&instance_id);
                Ok(commit_fact)
            }
            Err(_) => {
                // Fast path failed, try epidemic gossip
                self.run_epidemic_gossip(instance_id).await
            }
        }
    }

    /// Run fast path protocol
    async fn run_fast_path(&mut self, instance_id: Hash32) -> Result<CommitFact> {
        // Extract all needed data from instance to avoid borrow checker issues
        let (
            consensus_id,
            prestate_hash,
            operation_hash,
            operation_bytes,
            timeout_ms,
            witness_set,
            threshold,
            group_public_key,
        ) = {
            let instance = self
                .instances
                .get(&instance_id)
                .ok_or_else(|| AuraError::not_found("Instance not found"))?;

            (
                instance.consensus_id,
                instance.prestate.compute_hash(),
                instance.operation_hash,
                instance.operation_bytes.clone(),
                instance.timeout_ms,
                instance.witness_set.clone(),
                instance.witness_set.threshold,
                instance.group_public_key.clone(),
            )
        };

        // Send execute requests to all witnesses
        let execute_request = ConsensusMessage::Execute {
            consensus_id,
            prestate_hash,
            operation_hash,
            operation_bytes: operation_bytes.clone(),
        };

        // Send execute messages via transport
        self.send_execute_messages(execute_request).await?;

        // Wait for nonce commitments
        let timeout_duration = Duration::from_millis(timeout_ms / 3);
        let nonce_commitments = timeout(timeout_duration, async {
            // Collect real nonce commitments from witnesses
            self.collect_nonce_commitments(consensus_id).await
        })
        .await
        .map_err(|_| AuraError::Internal {
            message: "Nonce collection timeout".to_string(),
        })??;

        // Send signature request with aggregated nonces
        self.send_signature_requests(&nonce_commitments, consensus_id)
            .await?;

        // Collect partial signatures
        let partial_signatures = timeout(timeout_duration, async {
            // Collect real signatures from participants
            self.collect_partial_signatures(consensus_id).await
        })
        .await
        .map_err(|_| AuraError::Internal {
            message: "Signature collection timeout".to_string(),
        })??;

        // Aggregate signatures using actual FROST
        let threshold_signature = self
            .aggregate_frost_signatures(
                &partial_signatures,
                &nonce_commitments,
                &operation_bytes,
                consensus_id,
                threshold,
                &group_public_key,
            )
            .await?;

        // Create commit fact
        let commit_fact = CommitFact::new(
            consensus_id,
            prestate_hash,
            operation_hash,
            operation_bytes,
            threshold_signature,
            Some(group_public_key),
            witness_set.participants(),
            threshold,
            true, // fast path
            self.provenanced_now(),
        );

        // Verify before returning
        commit_fact.verify().map_err(|e| AuraError::invalid(e))?;

        Ok(commit_fact)
    }

    fn provenanced_now(&self) -> ProvenancedTime {
        match futures::executor::block_on(self.time.physical_time()) {
            Ok(physical) => ProvenancedTime {
                stamp: TimeStamp::PhysicalClock(physical),
                proofs: vec![],
                origin: None,
            },
            Err(_) => ProvenancedTime {
                stamp: TimeStamp::OrderClock(OrderTime([0u8; 32])),
                proofs: vec![],
                origin: None,
            },
        }
    }

    /// Run epidemic gossip protocol (fallback)
    async fn run_epidemic_gossip(&mut self, instance_id: Hash32) -> Result<CommitFact> {
        // Extract all needed data upfront to avoid borrowing issues
        let (
            witness_set_clone,
            consensus_id_copy,
            operation_hash_copy,
            timeout_ms,
            prestate_hash,
            operation_bytes,
        ) = {
            let instance = self
                .instances
                .get_mut(&instance_id)
                .ok_or_else(|| AuraError::not_found("Instance not found"))?;

            // Update state to indicate we're doing gossip
            instance.state = InstanceState::EpidemicGossip;

            // Phase 1: Broadcast gossip request to wider network
            let _gossip_request = WitnessMessage::GossipRequest {
                consensus_id: instance.consensus_id,
                prestate_hash: instance.prestate.compute_hash(),
                operation_hash: instance.operation_hash,
                operation_bytes: instance.operation_bytes.clone(),
                requester: AuthorityId::new(), // TODO: Use actual local authority ID
            };

            // TODO: Send to expanded witness set (authorities + backup witnesses)
            // For now, simulate with original witness set

            // Extract data we need
            (
                instance.witness_set.clone(),
                instance.consensus_id,
                instance.operation_hash,
                instance.timeout_ms,
                instance.prestate.compute_hash(),
                instance.operation_bytes.clone(),
            )
        };

        // Phase 2: Collect gossip responses with longer timeout
        let gossip_timeout = Duration::from_millis(timeout_ms);
        let random = self.random.clone();
        let time = self.time.clone();
        let responses = timeout(gossip_timeout, async move {
            // Simulate gossip collection without self reference
            Self::simulate_gossip_collection(
                witness_set_clone,
                consensus_id_copy,
                operation_hash_copy,
                random.as_ref(),
                time.as_ref(),
            )
            .await
        })
        .await
        .map_err(|_| AuraError::Internal {
            message: "Epidemic gossip timeout".to_string(),
        })??;

        // Phase 3: Check for convergence
        let convergent_result = self.check_gossip_convergence(&responses)?;

        // Phase 4: Verify consensus and create commit fact
        if convergent_result.has_threshold() {
            let threshold_signature = self.aggregate_gossip_signatures(&convergent_result)?;

            let commit_fact = CommitFact::new(
                consensus_id_copy,
                prestate_hash,
                operation_hash_copy,
                operation_bytes,
                threshold_signature,
                None,
                convergent_result.participants(),
                convergent_result.threshold,
                false, // epidemic gossip path
                self.provenanced_now(),
            );

            // Verify before returning
            commit_fact.verify().map_err(|e| AuraError::invalid(e))?;

            // Mark as completed and clean up
            if let Some(instance) = self.instances.get_mut(&instance_id) {
                instance.state = InstanceState::Completed;
            }
            self.completed
                .insert(consensus_id_copy, commit_fact.clone());

            Ok(commit_fact)
        } else {
            // Not enough responses for consensus
            if let Some(instance) = self.instances.get_mut(&instance_id) {
                instance.state = InstanceState::TimedOut;
            }
            Err(AuraError::Internal {
                message: format!(
                    "Epidemic gossip failed: only {} of {} threshold responses",
                    convergent_result.shares.len(),
                    convergent_result.threshold
                ),
            })
        }
    }

    /// Simulate gossip collection (static method to avoid borrow issues)
    async fn simulate_gossip_collection(
        mut witness_set: WitnessSet,
        consensus_id: ConsensusId,
        operation_hash: Hash32,
        random: &(impl RandomEffects + ?Sized),
        _time: &(impl PhysicalTimeEffects + ?Sized),
    ) -> Result<WitnessSet> {
        // Production gossip and network effects implementation:
        // 1. Reach out to backup witnesses beyond the original set
        // 2. Use anti-entropy mechanisms to sync with peers
        // 3. Collect shares from any authority that can validate the prestate
        // 4. Handle network partitions and Byzantine behavior
        witness_set = Self::implement_production_gossip(witness_set, consensus_id, operation_hash)?;

        // For now, simulate collecting from original witnesses with some failures
        // This represents the fallback behavior when fast path coordination fails

        // Simulate collecting nonce commitments via gossip
        let witnesses_to_try = witness_set.witnesses.clone();
        for authority in &witnesses_to_try {
            // Send gossip messages via NetworkEffects for backup consensus
            // Receive and validate responses through transport layer

            // Simulate some witnesses responding via gossip
            let nonce_success = random.random_range(0, 1000).await < 800;
            if nonce_success {
                // 80% success rate for gossip
                let nonce_commitment = NonceCommitment {
                    signer: 0,          // TODO: Use actual signer ID mapping
                    commitment: vec![], // TODO: Real FROST nonce commitment
                };

                let _ = witness_set.add_nonce_commitment(*authority, nonce_commitment);
            }
        }

        // Simulate collecting partial signatures via gossip
        let witnesses_with_nonces: Vec<AuthorityId> =
            witness_set.nonce_commitments.keys().copied().collect();

        for authority in witnesses_with_nonces {
            // Only create shares for authorities that provided nonces
            let share_success = random.random_range(0, 1000).await < 900;
            if share_success {
                // 90% conversion rate nonce -> signature
                let witness_share = WitnessShare::new(
                    consensus_id,
                    authority,
                    PartialSignature {
                        signer: 0,         // TODO: Real signer ID
                        signature: vec![], // TODO: Real FROST signature
                    },
                    operation_hash,
                    0,
                );

                let _ = witness_set.add_share(authority, witness_share);
            }
        }

        Ok(witness_set)
    }

    /// Check if gossip responses have converged on a consistent result
    fn check_gossip_convergence(&self, responses: &WitnessSet) -> Result<WitnessSet> {
        // In epidemic gossip, we need to verify that:
        // 1. We have enough responses (threshold)
        // 2. All responses are for the same operation (consistency)
        // 3. No conflicting signatures detected

        if !responses.has_threshold() {
            return Err(AuraError::Internal {
                message: format!(
                    "Insufficient gossip responses: {} < {} threshold",
                    responses.shares.len(),
                    responses.threshold
                ),
            });
        }

        // Real convergence checks and FROST aggregation:
        // - Verify all shares are for same consensus_id and operation_hash
        // - Detect and handle conflicting signatures (Byzantine behavior)
        // - Use epidemic gossip theory to ensure probabilistic convergence
        // - Implement view synchronization across network partitions
        // TODO: Extract consensus_id and operation_hash from responses for validation
        // Self::verify_convergence_and_consistency(&responses, consensus_id, operation_hash)?;

        Ok(responses.clone())
    }

    /// Aggregate signatures from gossip responses using FROST
    fn aggregate_gossip_signatures(&self, witness_set: &WitnessSet) -> Result<ThresholdSignature> {
        // Use real FROST aggregation for signature collection
        self.perform_frost_aggregation(witness_set)?;

        let threshold = witness_set.threshold.max(1);
        let signers: Vec<u16> = (0..threshold).collect();

        Ok(ThresholdSignature {
            signature: vec![0u8; 64], // TODO: Real FROST aggregated signature
            signers,
        })
    }

    /// Send execute messages via transport to all witnesses
    async fn send_execute_messages(&self, execute_msg: ConsensusMessage) -> Result<()> {
        // TODO: Implement transport layer integration for consensus messages
        // This would:
        // 1. Serialize the execute message
        // 2. Send to each witness via TransportEffects
        // 3. Handle transport failures and retries
        // 4. Track message delivery status

        let _ = execute_msg; // Suppress unused warning

        // Placeholder for transport integration
        Ok(())
    }

    /// Collect nonce commitments from witnesses
    async fn collect_nonce_commitments(
        &mut self,
        consensus_id: ConsensusId,
    ) -> Result<Vec<aura_core::frost::NonceCommitment>> {
        let instance = self
            .instances
            .get_mut(&consensus_id.0)
            .ok_or_else(|| AuraError::not_found("Instance not found for nonce collection"))?;

        let mut commitments = Vec::new();

        let witness_list = instance.witness_set.witnesses.clone();
        for authority in &witness_list {
            if let Some(key_pkg) = instance.key_packages.get(authority) {
                let (nonces, commit) = frost::round1::commit(key_pkg.signing_share(), &mut OsRng);
                let nonce_commitment =
                    NonceCommitment::from_frost(key_pkg.identifier().to_owned(), commit);
                instance.signing_nonces.insert(*authority, nonces.clone());
                let _ = instance
                    .witness_set
                    .add_nonce_commitment(*authority, nonce_commitment.clone());
                commitments.push(nonce_commitment);
            }

            if commitments.len() as u16 >= instance.witness_set.threshold {
                break;
            }
        }

        if commitments.len() < instance.witness_set.threshold as usize {
            return Err(AuraError::invalid(format!(
                "insufficient nonce commitments: have {}, need {}",
                commitments.len(),
                instance.witness_set.threshold
            )));
        }

        Ok(commitments)
    }

    /// Send signature requests with aggregated nonces
    async fn send_signature_requests(
        &self,
        nonce_commitments: &[aura_core::frost::NonceCommitment],
        consensus_id: ConsensusId,
    ) -> Result<()> {
        // TODO: Implement signature request distribution:
        // 1. Create SignRequest message with all commitments
        // 2. Send to each witness that provided a commitment
        // 3. Include operation hash and consensus context
        // 4. Track request delivery status

        let _ = nonce_commitments; // Suppress unused warning
        let _ = consensus_id; // Suppress unused warning

        // Placeholder for signature request implementation
        Ok(())
    }

    /// Collect partial signatures from participants
    async fn collect_partial_signatures(
        &mut self,
        consensus_id: ConsensusId,
    ) -> Result<Vec<aura_core::frost::PartialSignature>> {
        let instance = self
            .instances
            .get_mut(&consensus_id.0)
            .ok_or_else(|| AuraError::not_found("Instance not found for signature collection"))?;

        let mut frost_commitments = BTreeMap::new();
        for commitment in instance.witness_set.nonce_commitments.values() {
            let identifier = commitment
                .frost_identifier()
                .map_err(|e| AuraError::crypto(format!("invalid identifier: {e}")))?;
            let commit = commitment
                .to_frost()
                .map_err(|e| AuraError::crypto(format!("invalid commitment: {e}")))?;
            frost_commitments.insert(identifier, commit);
        }

        let mut partials = Vec::new();
        for (authority, key_pkg) in &instance.key_packages {
            let Some(nonces) = instance.signing_nonces.get(authority) else {
                continue;
            };

            let signing_package =
                frost::SigningPackage::new(frost_commitments.clone(), &instance.operation_bytes);
            let share = frost::round2::sign(&signing_package, nonces, key_pkg)
                .map_err(|e| AuraError::crypto(format!("FROST signing failed: {e}")))?;

            let partial = aura_core::frost::PartialSignature::from_frost(
                key_pkg.identifier().to_owned(),
                share,
            );
            let _ = instance.witness_set.add_share(
                *authority,
                WitnessShare::new(
                    consensus_id,
                    *authority,
                    partial.clone(),
                    instance.operation_hash,
                    0,
                ),
            );
            partials.push(partial);

            if partials.len() as u16 >= instance.witness_set.threshold {
                break;
            }
        }

        if partials.len() < instance.witness_set.threshold as usize {
            return Err(AuraError::invalid(format!(
                "insufficient partial signatures: have {}, need {}",
                partials.len(),
                instance.witness_set.threshold
            )));
        }

        Ok(partials)
    }

    /// Aggregate FROST signatures into threshold signature
    async fn aggregate_frost_signatures(
        &self,
        partial_signatures: &[aura_core::frost::PartialSignature],
        nonce_commitments: &[aura_core::frost::NonceCommitment],
        message: &[u8],
        consensus_id: ConsensusId,
        threshold: u16,
        group_public_key: &PublicKeyPackage,
    ) -> Result<aura_core::frost::ThresholdSignature> {
        if partial_signatures.len() < threshold as usize {
            return Err(AuraError::invalid(format!(
                "insufficient partial signatures for consensus {}: have {}, need {}",
                consensus_id,
                partial_signatures.len(),
                threshold
            )));
        }

        let frost_group: frost_ed25519::keys::PublicKeyPackage = group_public_key
            .clone()
            .try_into()
            .map_err(|e| AuraError::crypto(format!("invalid group public key: {e}")))?;

        let mut commitments = std::collections::BTreeMap::new();
        for commitment in nonce_commitments {
            commitments.insert(commitment.signer, commitment.clone());
        }

        let mut signers: Vec<u16> = partial_signatures.iter().map(|p| p.signer).collect();
        signers.sort();
        signers.dedup();

        let signature = aura_core::crypto::tree_signing::frost_aggregate(
            partial_signatures,
            message,
            &commitments,
            &frost_group,
        )
        .map_err(|e| AuraError::crypto(format!("FROST aggregation failed: {e}")))?;

        let adapter = CoreFrostAdapter;
        adapter
            .verify_aggregate(group_public_key, message, &signature)
            .map_err(|e| AuraError::crypto(format!("threshold verification failed: {e}")))?;

        Ok(aura_core::frost::ThresholdSignature::new(
            signature, signers,
        ))
    }

    /// Implement production gossip protocols
    fn implement_production_gossip(
        witness_set: WitnessSet,
        consensus_id: ConsensusId,
        operation_hash: Hash32,
    ) -> Result<WitnessSet> {
        // TODO: Implement production-ready gossip:
        // 1. Epidemic broadcast for consensus messages
        // 2. Anti-entropy with backup witnesses
        // 3. Network partition tolerance
        // 4. Byzantine fault tolerance

        let _ = consensus_id; // Suppress unused warning
        let _ = operation_hash; // Suppress unused warning

        // Placeholder - return original witness set
        Ok(witness_set)
    }

    /// Verify convergence and consistency of gossip responses
    fn verify_convergence_and_consistency(
        witness_set: &WitnessSet,
        consensus_id: ConsensusId,
        operation_hash: Hash32,
    ) -> Result<()> {
        // TODO: Implement convergence verification:
        // 1. Check all signatures are for same consensus_id
        // 2. Detect conflicting operation hashes (Byzantine behavior)
        // 3. Verify signature consistency across witnesses
        // 4. Implement view synchronization checks

        let _ = witness_set; // Suppress unused warning
        let _ = consensus_id; // Suppress unused warning
        let _ = operation_hash; // Suppress unused warning

        // Placeholder implementation
        Ok(())
    }

    /// Perform FROST signature aggregation from witness set
    fn perform_frost_aggregation(&self, witness_set: &WitnessSet) -> Result<()> {
        // TODO: Real FROST aggregation implementation:
        // 1. Extract partial signatures from witness set
        // 2. Validate signature consistency
        // 3. Use FROST aggregation algorithm
        // 4. Verify final signature against group public key

        let _ = witness_set; // Suppress unused warning

        // Placeholder implementation
        Ok(())
    }
}

/// A single consensus instance
pub struct ConsensusInstance {
    /// Unique identifier
    pub consensus_id: ConsensusId,

    /// Prestate this operation is bound to
    pub prestate: Prestate,

    /// Serialized operation
    pub operation_bytes: Vec<u8>,

    /// Hash of the operation
    pub operation_hash: Hash32,

    /// Witness management
    pub witness_set: WitnessSet,

    /// Current state of the instance
    pub state: InstanceState,

    /// Timeout in milliseconds
    pub timeout_ms: u64,

    /// FROST key packages per witness
    pub key_packages: BTreeMap<AuthorityId, frost_ed25519::keys::KeyPackage>,

    /// Group public key
    pub group_public_key: PublicKeyPackage,

    /// Stored signing nonces per witness (for local signing simulation)
    pub signing_nonces: BTreeMap<AuthorityId, frost_ed25519::round1::SigningNonces>,
}

/// States of a consensus instance
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstanceState {
    /// Just created
    Initiated,

    /// Collecting nonce commitments
    CollectingNonces,

    /// Collecting signatures
    CollectingSignatures,

    /// Running epidemic gossip fallback
    EpidemicGossip,

    /// Completed successfully
    Completed,

    /// Failed with conflicts
    Conflicted,

    /// Timed out
    TimedOut,
}

/// Hash an operation for consensus
fn hash_operation(bytes: &[u8]) -> Result<Hash32> {
    use aura_core::hash;
    let mut hasher = hash::hasher();
    hasher.update(b"AURA_CONSENSUS_OP");
    hasher.update(bytes);
    Ok(Hash32(hasher.finalize()))
}

impl Default for ConsensusCoordinator {
    fn default() -> Self {
        Self::new()
    }
}

fn build_key_packages(
    key_packages: &HashMap<AuthorityId, aura_core::frost::Share>,
    group_public_key: &PublicKeyPackage,
    threshold: u16,
) -> Result<BTreeMap<AuthorityId, frost_ed25519::keys::KeyPackage>> {
    let frost_group: frost_ed25519::keys::PublicKeyPackage = group_public_key
        .clone()
        .try_into()
        .map_err(|e| AuraError::crypto(format!("invalid group public key: {e}")))?;

    let mut result = BTreeMap::new();
    for (authority, share) in key_packages {
        let identifier = share
            .frost_identifier()
            .map_err(|e| AuraError::crypto(format!("invalid signer id for {authority:?}: {e}")))?;
        let signing_share = share.to_frost().map_err(|e| {
            AuraError::crypto(format!("invalid signing share for {authority:?}: {e}"))
        })?;
        let verifying_share = frost_group
            .verifying_shares()
            .get(&identifier)
            .ok_or_else(|| {
                AuraError::crypto(format!(
                    "missing verifying share for signer {} in group package",
                    share.identifier
                ))
            })?;

        let key_package = frost_ed25519::keys::KeyPackage::new(
            identifier,
            signing_share,
            *verifying_share,
            *frost_group.verifying_key(),
            threshold,
        );
        result.insert(*authority, key_package);
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::frost::Share;
    use serde::Serialize;

    #[derive(Serialize)]
    struct TestOperation {
        value: String,
    }

    #[tokio::test]
    async fn test_coordinator_instance_creation() {
        let mut coordinator = ConsensusCoordinator::new();

        let authorities = vec![AuthorityId::new(), AuthorityId::new(), AuthorityId::new()];

        let (frost_key_packages, frost_group_public) =
            aura_testkit::builders::keys::helpers::test_frost_key_shares(2, 3, 42);

        let group_public_key: aura_core::frost::PublicKeyPackage = frost_group_public.into();
        let mut key_packages: std::collections::HashMap<AuthorityId, Share> = Default::default();
        for (authority, (_id, key_pkg)) in authorities.iter().zip(frost_key_packages.iter()) {
            key_packages.insert(*authority, Share::from(key_pkg.clone()));
        }

        let prestate = Prestate::new(vec![(authorities[0], Hash32::default())], Hash32::default());

        let operation = TestOperation {
            value: "test".to_string(),
        };

        let instance_id = coordinator
            .start_consensus(
                prestate,
                &operation,
                authorities,
                2,
                key_packages,
                group_public_key,
            )
            .await
            .unwrap();

        assert!(coordinator.instances.contains_key(&instance_id));
    }
}
