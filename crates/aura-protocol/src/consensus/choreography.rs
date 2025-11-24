//! Consensus Choreography
//!
//! This module implements the choreographic protocol for Aura Consensus,
//! with support for FROST threshold signatures.
//!
//! ## FROST Integration
//!
//! This module coordinates FROST threshold cryptography through proper architectural boundaries:
//! Layer 4 (orchestration) → crypto adapter (core FROST primitives) → Layer 1 types.
//!
//! - **Nonce Generation**: via effects (placeholder until wired)
//! - **Partial Signing**: via effects (placeholder until wired)
//! - **Aggregation**: via consensus FROST adapter (core primitives)
//! - **Verification**: Verifies aggregated signatures against group public key
//!
//! ## Architecture
//!
//! The consensus protocol follows a 5-phase choreography:
//! 1. **Execute**: Coordinator initiates consensus with prestate and operation
//! 2. **Nonce Commit**: Witnesses generate nonces and send commitments
//! 3. **Sign Request**: Coordinator aggregates nonces and requests signatures
//! 4. **Sign Share**: Witnesses create partial signatures
//! 5. **Result**: Coordinator aggregates signatures and broadcasts commit fact
//!
//! ## Dependencies
//!
//! - Uses `aura-core` FROST types only
//! - Depends on `frost-ed25519` through the adapter
//! - Requires properly generated FROST key shares from DKG/resharing ceremonies

use super::{CommitFact, ConsensusId};
use aura_core::crypto::tree_signing::frost_aggregate;
use aura_core::effects::{PhysicalTimeEffects, RandomEffects};
use aura_core::frost::{NonceCommitment, PartialSignature, PublicKeyPackage, Share};
use aura_core::{AuraError, AuthorityId, Hash32, Result};
use aura_macros::choreography;
use frost_ed25519 as frost;
use rand::rngs::OsRng;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};

/// Consensus protocol messages
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ConsensusMessage {
    /// Execute request from coordinator
    Execute {
        consensus_id: ConsensusId,
        prestate_hash: Hash32,
        operation_hash: Hash32,
        operation_bytes: Vec<u8>,
    },

    /// Nonce commitment from witness
    NonceCommit {
        consensus_id: ConsensusId,
        commitment: NonceCommitment,
    },

    /// Aggregated nonces for signing
    SignRequest {
        consensus_id: ConsensusId,
        aggregated_nonces: Vec<NonceCommitment>,
    },

    /// Partial signature from witness
    SignShare {
        consensus_id: ConsensusId,
        share: PartialSignature,
    },

    /// Final consensus result
    ConsensusResult { commit_fact: CommitFact },

    /// Conflict detected
    Conflict {
        consensus_id: ConsensusId,
        conflicts: Vec<Hash32>,
    },
}

/// Consensus choreography configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsensusChoreographyConfig {
    /// Consensus instance ID
    pub consensus_id: ConsensusId,

    /// Required threshold
    pub threshold: u16,

    /// Selected witnesses
    pub witnesses: Vec<AuthorityId>,

    /// Timeout in milliseconds
    pub timeout_ms: u64,
}

// Define the consensus choreography using aura-macros
choreography! {
    #[namespace = "aura_consensus"]
    protocol AuraConsensus {
        roles: Coordinator, Witness[n];

        // Phase 1: Initiate consensus
        Coordinator[guard_capability = "initiate_consensus", flow_cost = 100]
        -> Witness[*]: Execute(ConsensusMessage);

        // Phase 2: Collect nonce commitments
        Witness[*][guard_capability = "witness_nonce", flow_cost = 50]
        -> Coordinator: NonceCommit(ConsensusMessage);

        // Phase 3: Request signatures with aggregated nonces
        Coordinator[guard_capability = "aggregate_nonces", flow_cost = 75]
        -> Witness[*]: SignRequest(ConsensusMessage);

        // Phase 4: Collect partial signatures
        Witness[*][guard_capability = "witness_sign", flow_cost = 50]
        -> Coordinator: SignShare(ConsensusMessage);

        // Phase 5: Broadcast result
        Coordinator[guard_capability = "finalize_consensus", flow_cost = 100,
                    journal_facts = "consensus_complete"]
        -> Witness[*]: ConsensusResult(ConsensusMessage);
    }
}

/// Coordinator role implementation
pub struct CoordinatorRole {
    pub authority_id: AuthorityId,
    pub config: ConsensusChoreographyConfig,
    pub collected_nonces: HashMap<AuthorityId, NonceCommitment>,
    pub collected_shares: HashMap<AuthorityId, PartialSignature>,
}

impl CoordinatorRole {
    /// Create a new coordinator role
    pub fn new(authority_id: AuthorityId, config: ConsensusChoreographyConfig) -> Self {
        Self {
            authority_id,
            config,
            collected_nonces: HashMap::new(),
            collected_shares: HashMap::new(),
        }
    }

    /// Create execute message
    pub fn create_execute_message(
        &self,
        prestate_hash: Hash32,
        operation_hash: Hash32,
        operation_bytes: Vec<u8>,
    ) -> ConsensusMessage {
        ConsensusMessage::Execute {
            consensus_id: self.config.consensus_id,
            prestate_hash,
            operation_hash,
            operation_bytes,
        }
    }

    /// Handle nonce commitment from witness
    pub fn handle_nonce_commit(
        &mut self,
        witness: AuthorityId,
        commitment: NonceCommitment,
    ) -> Result<()> {
        if !self.config.witnesses.contains(&witness) {
            return Err(AuraError::invalid("Unknown witness".to_string()));
        }

        self.collected_nonces.insert(witness, commitment);
        Ok(())
    }

    /// Check if we have enough nonces
    pub fn has_nonce_threshold(&self) -> bool {
        self.collected_nonces.len() >= self.config.threshold as usize
    }

    /// Create sign request with aggregated nonces
    pub fn create_sign_request(&self) -> ConsensusMessage {
        let aggregated_nonces: Vec<_> = self.collected_nonces.values().cloned().collect();

        ConsensusMessage::SignRequest {
            consensus_id: self.config.consensus_id,
            aggregated_nonces,
        }
    }

    /// Handle signature share from witness
    pub fn handle_sign_share(
        &mut self,
        witness: AuthorityId,
        share: PartialSignature,
    ) -> Result<()> {
        if !self.config.witnesses.contains(&witness) {
            return Err(AuraError::invalid("Unknown witness".to_string()));
        }

        self.collected_shares.insert(witness, share);
        Ok(())
    }

    /// Check if we have enough signatures
    pub fn has_signature_threshold(&self) -> bool {
        self.collected_shares.len() >= self.config.threshold as usize
    }

    /// Aggregate signatures and create commit fact
    ///
    /// TODO: Replace with proper FROST aggregation via the consensus crypto adapter.
    /// Should delegate through Layer 3 (effects) to Layer 1 (core primitives).
    pub fn create_commit_fact(
        &self,
        prestate_hash: Hash32,
        operation_hash: Hash32,
        operation_bytes: Vec<u8>,
        group_public_key: &PublicKeyPackage,
        timestamp: aura_core::time::ProvenancedTime,
    ) -> Result<CommitFact> {
        let participants: Vec<_> = self.collected_shares.keys().cloned().collect();

        if self.collected_shares.len() < self.config.threshold as usize {
            return Err(AuraError::invalid(
                "Insufficient signature shares for threshold".to_string(),
            ));
        }

        let frost_group: frost::keys::PublicKeyPackage = group_public_key
            .clone()
            .try_into()
            .map_err(|e| AuraError::crypto(format!("invalid group public key: {e}")))?;

        let mut commitments: BTreeMap<u16, NonceCommitment> = BTreeMap::new();
        for commitment in self.collected_nonces.values() {
            commitments.insert(commitment.signer, commitment.clone());
        }

        let partials: Vec<_> = self.collected_shares.values().cloned().collect();
        let mut signers: Vec<u16> = partials.iter().map(|s| s.signer).collect();
        signers.sort();
        signers.dedup();

        let signature = frost_aggregate(&partials, &operation_bytes, &commitments, &frost_group)
            .map_err(|e| AuraError::crypto(format!("threshold aggregation failed: {e}")))?;

        let threshold_signature =
            aura_core::frost::ThresholdSignature::new(signature, signers.clone());

        let commit_fact = CommitFact::new(
            self.config.consensus_id,
            prestate_hash,
            operation_hash,
            operation_bytes,
            threshold_signature,
            Some(group_public_key.clone()),
            participants,
            self.config.threshold,
            true, // fast path
            timestamp,
        );

        Ok(commit_fact)
    }
}

/// Witness role implementation
pub struct WitnessRole {
    pub authority_id: AuthorityId,
    pub key_package: frost::keys::KeyPackage,
    pub active_instances: HashMap<ConsensusId, WitnessInstance>,
}

/// Witness state for a consensus instance
pub struct WitnessInstance {
    pub prestate_hash: Hash32,
    pub operation_hash: Hash32,
    pub operation_bytes: Vec<u8>,
    pub signing_nonces: Option<frost::round1::SigningNonces>,
    pub nonce_commitment: Option<NonceCommitment>,
    pub partial_signature: Option<PartialSignature>,
}

impl WitnessRole {
    /// Create a new witness role
    pub fn new(authority_id: AuthorityId, key_package: frost::keys::KeyPackage) -> Self {
        Self {
            authority_id,
            key_package,
            active_instances: HashMap::new(),
        }
    }

    /// Handle execute request
    pub fn handle_execute(
        &mut self,
        consensus_id: ConsensusId,
        prestate_hash: Hash32,
        operation_hash: Hash32,
        operation_bytes: Vec<u8>,
    ) -> Result<ConsensusMessage> {
        // Verify prestate matches our view
        self.verify_prestate_matches_view(prestate_hash)?;

        let (signing_nonces, signing_commitment) =
            frost::round1::commit(self.key_package.signing_share(), &mut OsRng);
        let nonce_commitment = aura_core::frost::NonceCommitment::from_frost(
            self.key_package.identifier().to_owned(),
            signing_commitment,
        );

        let instance = WitnessInstance {
            prestate_hash,
            operation_hash,
            operation_bytes,
            signing_nonces: Some(signing_nonces),
            nonce_commitment: Some(nonce_commitment.clone()),
            partial_signature: None,
        };

        self.active_instances.insert(consensus_id, instance);

        Ok(ConsensusMessage::NonceCommit {
            consensus_id,
            commitment: nonce_commitment,
        })
    }

    /// Handle sign request via proper FROST delegation
    ///
    /// TODO: Replace with proper adapter-based delegation following architectural boundaries.
    /// Should orchestrate through effects → crypto adapter → core primitives.
    pub fn handle_sign_request(
        &mut self,
        consensus_id: ConsensusId,
        aggregated_nonces: Vec<NonceCommitment>,
    ) -> Result<ConsensusMessage> {
        let instance = self
            .active_instances
            .get_mut(&consensus_id)
            .ok_or_else(|| AuraError::not_found("Unknown consensus instance"))?;

        let signing_nonces = instance
            .signing_nonces
            .clone()
            .ok_or_else(|| AuraError::invalid("missing signing nonces for witness"))?;

        let mut frost_commitments = BTreeMap::new();
        for commitment in &aggregated_nonces {
            let identifier = commitment
                .frost_identifier()
                .map_err(|e| AuraError::crypto(format!("invalid signer identifier: {e}")))?;
            let frost_commitment = commitment
                .to_frost()
                .map_err(|e| AuraError::crypto(format!("invalid nonce commitment: {e}")))?;
            frost_commitments.insert(identifier, frost_commitment);
        }

        let signing_package =
            frost::SigningPackage::new(frost_commitments, &instance.operation_bytes);
        let signature_share =
            frost::round2::sign(&signing_package, &signing_nonces, &self.key_package)
                .map_err(|e| AuraError::crypto(format!("FROST signing failed: {e}")))?;

        let signer = self
            .key_package
            .identifier()
            .serialize()
            .first()
            .copied()
            .unwrap_or_default();
        let partial_signature =
            PartialSignature::from_frost(self.key_package.identifier().to_owned(), signature_share);

        instance.partial_signature = Some(partial_signature.clone());

        Ok(ConsensusMessage::SignShare {
            consensus_id,
            share: partial_signature,
        })
    }

    /// Handle consensus result
    pub fn handle_consensus_result(&mut self, commit_fact: &CommitFact) -> Result<()> {
        // Clean up instance
        self.active_instances.remove(&commit_fact.consensus_id);

        // Verify the choreography result
        self.verify_choreography_result(commit_fact)?;

        Ok(())
    }

    /// Verify that the prestate hash matches our current view
    fn verify_prestate_matches_view(&self, prestate_hash: Hash32) -> Result<()> {
        // TODO: Implement actual prestate verification by:
        // 1. Getting our current state hash from the journal
        // 2. Comparing against the provided prestate_hash
        // 3. Ensuring state consistency across witnesses

        // For now, perform basic validation
        if prestate_hash == Hash32::default() {
            return Err(AuraError::invalid("Invalid prestate hash"));
        }

        // Placeholder - in production this would:
        // - Retrieve current authority state hash
        // - Compare with prestate_hash
        // - Reject if mismatch (state divergence)
        Ok(())
    }

    /// Verify the choreography result is valid
    fn verify_choreography_result(&self, commit_fact: &CommitFact) -> Result<()> {
        // TODO: Implement comprehensive choreography result verification:
        // 1. Validate threshold signature using group public key
        // 2. Ensure participant count meets threshold requirements
        // 3. Verify consensus_id matches expected values
        // 4. Check operation integrity

        // Basic validation for now
        if commit_fact.participants.len() < commit_fact.threshold as usize {
            return Err(AuraError::invalid("Insufficient participants in result"));
        }

        if commit_fact.threshold_signature.signature.is_empty() {
            return Err(AuraError::invalid("Empty threshold signature in result"));
        }

        // TODO: Add full FROST signature verification once key packages are integrated
        Ok(())
    }
}

/// Integration with effect system using real FROST threshold signatures
///
/// This function runs the complete consensus choreography with real FROST cryptography.
/// It requires properly generated FROST key shares and performs actual threshold signing.
///
/// # Parameters
///
/// * `prestate_hash` - Hash of the pre-state before operation
/// * `operation_hash` - Hash of the operation being committed
/// * `operation_bytes` - Serialized operation data
/// * `witnesses` - List of witness authorities participating in consensus
/// * `threshold` - Required number of signatures (M in M-of-N)
/// * `key_packages` - Map of witness IDs to their FROST key packages (from DKG)
/// * `group_public_key` - FROST group public key for aggregation and verification
///
/// # Returns
///
/// A `CommitFact` with a verified FROST threshold signature
///
/// # Errors
///
/// Returns error if:
/// - Insufficient witnesses or invalid threshold
/// - FROST cryptographic operations fail
/// - Signature aggregation or verification fails
/// - Not enough witnesses provide commitments or signatures
#[allow(clippy::too_many_arguments)]
pub async fn run_consensus_choreography(
    prestate_hash: Hash32,
    operation_hash: Hash32,
    operation_bytes: Vec<u8>,
    witnesses: Vec<AuthorityId>,
    threshold: u16,
    key_packages: HashMap<AuthorityId, Share>,
    group_public_key: PublicKeyPackage,
    random: &(impl RandomEffects + ?Sized),
    time: &(impl PhysicalTimeEffects + ?Sized),
) -> Result<CommitFact> {
    if witnesses.is_empty() {
        return Err(AuraError::invalid(
            "Consensus requires at least one witness",
        ));
    }

    let threshold = threshold.max(1).min(witnesses.len() as u16);
    let frost_key_packages = build_frost_key_packages(&key_packages, &group_public_key, threshold)?;

    for witness in &witnesses {
        if !frost_key_packages.contains_key(witness) {
            return Err(AuraError::crypto(format!(
                "missing key package for witness {}",
                witness
            )));
        }
    }

    // Replace placeholder FROST nonce generation with effect-driven randomness
    let nonce: u64 = random.random_u64().await;
    let consensus_id = ConsensusId::new(prestate_hash, operation_hash, nonce);

    let config = ConsensusChoreographyConfig {
        consensus_id,
        threshold,
        witnesses: witnesses.clone(),
        timeout_ms: 30000,
    };

    let mut coordinator = CoordinatorRole::new(witnesses[0], config.clone());
    let mut witness_roles: HashMap<AuthorityId, WitnessRole> = witnesses
        .iter()
        .map(|id| {
            let key_pkg = frost_key_packages
                .get(id)
                .expect("validated above to exist")
                .clone();
            (*id, WitnessRole::new(*id, key_pkg))
        })
        .collect();

    // Phase 1: Execute -> NonceCommit
    // Generate the initial execute payload (transport integration pending)
    let _execute_message =
        coordinator.create_execute_message(prestate_hash, operation_hash, operation_bytes.clone());

    for (idx, witness_id) in config.witnesses.iter().enumerate() {
        let witness = witness_roles
            .get_mut(witness_id)
            .ok_or_else(|| AuraError::not_found("Witness not found"))?;

        let nonce_msg = witness.handle_execute(
            config.consensus_id,
            prestate_hash,
            operation_hash,
            operation_bytes.clone(),
        )?;

        if let ConsensusMessage::NonceCommit {
            consensus_id: _,
            commitment,
        } = nonce_msg
        {
            coordinator.handle_nonce_commit(*witness_id, commitment)?;
        }
    }

    if !coordinator.has_nonce_threshold() {
        return Err(AuraError::invalid(
            "Insufficient nonce commitments for consensus",
        ));
    }

    // Phase 2: SignRequest (using real FROST signing)
    let sign_request = coordinator.create_sign_request();
    let aggregated_nonces = match &sign_request {
        ConsensusMessage::SignRequest {
            aggregated_nonces, ..
        } => aggregated_nonces.clone(),
        _ => Vec::new(),
    };

    for witness_id in &config.witnesses {
        let witness = witness_roles
            .get_mut(witness_id)
            .ok_or_else(|| AuraError::not_found("Witness not found"))?;

        let sign_msg =
            witness.handle_sign_request(config.consensus_id, aggregated_nonces.clone())?;

        if let ConsensusMessage::SignShare {
            consensus_id: _,
            share,
        } = sign_msg
        {
            coordinator.handle_sign_share(*witness_id, share)?;
        }
    }

    if !coordinator.has_signature_threshold() {
        return Err(AuraError::invalid(
            "Insufficient signature shares for consensus",
        ));
    }

    // Phase 3: Commit + broadcast result locally (using real FROST aggregation)
    let timestamp = time
        .physical_time()
        .await
        .map(|t| aura_core::time::ProvenancedTime {
            stamp: aura_core::time::TimeStamp::PhysicalClock(t),
            proofs: vec![],
            origin: Some(coordinator.authority_id),
        })
        .unwrap_or_else(|_| aura_core::time::ProvenancedTime {
            stamp: aura_core::time::TimeStamp::OrderClock(aura_core::time::OrderTime([0u8; 32])),
            proofs: vec![],
            origin: Some(coordinator.authority_id),
        });
    let commit_fact = coordinator.create_commit_fact(
        prestate_hash,
        operation_hash,
        operation_bytes.clone(),
        &group_public_key,
        timestamp,
    )?;

    commit_fact.verify().map_err(|e| AuraError::invalid(e))?;

    let result_message = ConsensusMessage::ConsensusResult {
        commit_fact: commit_fact.clone(),
    };

    for witness_id in &config.witnesses {
        if let Some(witness) = witness_roles.get_mut(witness_id) {
            witness.handle_consensus_result(&commit_fact)?;
        }
        // Local broadcast noop; if we had NetworkEffects we'd send `result_message`
        let _ = &result_message;
    }

    Ok(commit_fact)
}

fn build_frost_key_packages(
    key_packages: &HashMap<AuthorityId, Share>,
    group_public_key: &PublicKeyPackage,
    threshold: u16,
) -> Result<HashMap<AuthorityId, frost::keys::KeyPackage>> {
    let frost_group: frost::keys::PublicKeyPackage = group_public_key
        .clone()
        .try_into()
        .map_err(|e| AuraError::crypto(format!("invalid group public key: {e}")))?;

    let mut result = HashMap::new();
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

        let key_package = frost::keys::KeyPackage::new(
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

    #[test]
    fn test_coordinator_role() {
        let config = ConsensusChoreographyConfig {
            consensus_id: ConsensusId(Hash32::default()),
            threshold: 2,
            witnesses: vec![AuthorityId::new(), AuthorityId::new(), AuthorityId::new()],
            timeout_ms: 30000,
        };

        let mut coordinator = CoordinatorRole::new(AuthorityId::new(), config);

        assert!(!coordinator.has_nonce_threshold());

        // Add nonces (collect witnesses first to avoid borrow checker issues)
        let witnesses_to_commit: Vec<_> = coordinator.config.witnesses[..2].to_vec();
        for witness in witnesses_to_commit {
            coordinator
                .handle_nonce_commit(
                    witness,
                    NonceCommitment {
                        signer: 0,
                        commitment: vec![],
                    },
                )
                .unwrap();
        }

        assert!(coordinator.has_nonce_threshold());
    }
}
