//! Consensus Choreography
//!
//! This module implements the choreographic protocol for Aura Consensus,
//! integrating with FROST threshold signatures to produce consensus proofs.

use super::{CommitFact, ConsensusId};
use aura_core::frost::{NonceCommitment, PartialSignature, ThresholdSignature};
use aura_core::{hash, AuraError, AuthorityId, Hash32, Result};
use aura_macros::choreography;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

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
    pub fn create_commit_fact(
        &self,
        prestate_hash: Hash32,
        operation_hash: Hash32,
        operation_bytes: Vec<u8>,
    ) -> Result<CommitFact> {
        let participants: Vec<_> = self.collected_shares.keys().cloned().collect();

        let signers: Vec<u16> = self
            .collected_shares
            .values()
            .map(|s| s.signer)
            .collect();

        // Deterministic placeholder signature bound to content and signer IDs
        let mut hasher = hash::hasher();
        hasher.update(b"AURA_CONSENSUS_AGG_SIG");
        hasher.update(&prestate_hash.0);
        hasher.update(&operation_hash.0);
        for signer in &signers {
            hasher.update(&signer.to_le_bytes());
        }
        let mut sig = hasher.finalize().to_vec();
        if sig.len() < 64 {
            while sig.len() < 64 {
                sig.extend_from_slice(&operation_hash.0);
            }
            sig.truncate(64);
        }

        let threshold_signature = ThresholdSignature {
            signature: sig,
            signers,
        };

        let commit_fact = CommitFact::new(
            self.config.consensus_id,
            prestate_hash,
            operation_hash,
            operation_bytes,
            threshold_signature,
            participants,
            self.config.threshold,
            true, // fast path
        );

        Ok(commit_fact)
    }
}

/// Witness role implementation
pub struct WitnessRole {
    pub authority_id: AuthorityId,
    pub active_instances: HashMap<ConsensusId, WitnessInstance>,
}

/// Witness state for a consensus instance
pub struct WitnessInstance {
    pub prestate_hash: Hash32,
    pub operation_hash: Hash32,
    pub signer: u16,
    pub nonce_commitment: Option<NonceCommitment>,
    pub partial_signature: Option<PartialSignature>,
}

impl WitnessRole {
    /// Create a new witness role
    pub fn new(authority_id: AuthorityId) -> Self {
        Self {
            authority_id,
            active_instances: HashMap::new(),
        }
    }

    /// Handle execute request
    pub fn handle_execute(
        &mut self,
        consensus_id: ConsensusId,
        prestate_hash: Hash32,
        operation_hash: Hash32,
        signer: u16,
    ) -> Result<ConsensusMessage> {
        // TODO: Verify prestate matches our view

        let nonce_commitment = derive_nonce_commitment(consensus_id, self.authority_id, signer);

        let instance = WitnessInstance {
            prestate_hash,
            operation_hash,
            signer,
            nonce_commitment: Some(nonce_commitment.clone()),
            partial_signature: None,
        };

        self.active_instances.insert(consensus_id, instance);

        Ok(ConsensusMessage::NonceCommit {
            consensus_id,
            commitment: nonce_commitment,
        })
    }

    /// Handle sign request
    pub fn handle_sign_request(
        &mut self,
        consensus_id: ConsensusId,
        _aggregated_nonces: Vec<NonceCommitment>,
    ) -> Result<ConsensusMessage> {
        let instance = self
            .active_instances
            .get_mut(&consensus_id)
            .ok_or_else(|| AuraError::not_found("Unknown consensus instance"))?;

        // TODO: Generate real partial signature using FROST
        let partial_signature =
            derive_partial_signature(consensus_id, self.authority_id, instance.signer, instance.prestate_hash, instance.operation_hash);

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

        // TODO: Verify the result

        Ok(())
    }
}

/// Integration with effect system
pub async fn run_consensus_choreography(
    prestate_hash: Hash32,
    operation_hash: Hash32,
    operation_bytes: Vec<u8>,
    witnesses: Vec<AuthorityId>,
    threshold: u16,
) -> Result<CommitFact> {
    if witnesses.is_empty() {
        return Err(AuraError::invalid("Consensus requires at least one witness"));
    }

    let threshold = threshold.max(1).min(witnesses.len() as u16);
    let nonce: u64 = rand::thread_rng().next_u64();
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
        .map(|id| (*id, WitnessRole::new(*id)))
        .collect();

    // Phase 1: Execute -> NonceCommit
    let execute_message = coordinator.create_execute_message(
        prestate_hash,
        operation_hash,
        operation_bytes.clone(),
    );

    for (idx, witness_id) in config.witnesses.iter().enumerate() {
        let witness = witness_roles
            .get_mut(witness_id)
            .ok_or_else(|| AuraError::not_found("Witness not found"))?;

        let nonce_msg = witness.handle_execute(
            config.consensus_id,
            prestate_hash,
            operation_hash,
            (idx + 1) as u16,
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

    // Phase 2: SignRequest
    let sign_request = coordinator.create_sign_request();
    let aggregated_nonces = match &sign_request {
        ConsensusMessage::SignRequest {
            aggregated_nonces, ..
        } => aggregated_nonces.clone(),
        _ => Vec::new(),
    };

    for (idx, witness_id) in config.witnesses.iter().enumerate() {
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

    // Phase 3: Commit + broadcast result locally
    let commit_fact = coordinator.create_commit_fact(
        prestate_hash,
        operation_hash,
        operation_bytes.clone(),
    )?;

    commit_fact
        .verify()
        .map_err(|e| AuraError::invalid(e))?;

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

fn derive_nonce_commitment(
    consensus_id: ConsensusId,
    authority: AuthorityId,
    signer: u16,
) -> NonceCommitment {
    let mut hasher = hash::hasher();
    hasher.update(b"AURA_CONS_NONCE");
    hasher.update(&consensus_id.0 .0);
    hasher.update(&authority.to_bytes());
    hasher.update(&signer.to_le_bytes());
    let commitment = hasher.finalize().to_vec();

    NonceCommitment { signer, commitment }
}

fn derive_partial_signature(
    consensus_id: ConsensusId,
    authority: AuthorityId,
    signer: u16,
    prestate_hash: Hash32,
    operation_hash: Hash32,
) -> PartialSignature {
    let mut hasher = hash::hasher();
    hasher.update(b"AURA_CONS_PARTIAL_SIG");
    hasher.update(&consensus_id.0 .0);
    hasher.update(&prestate_hash.0);
    hasher.update(&operation_hash.0);
    hasher.update(&authority.to_bytes());
    hasher.update(&signer.to_le_bytes());
    let signature = hasher.finalize().to_vec();

    PartialSignature { signer, signature }
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
                .handle_nonce_commit(witness, NonceCommitment {
                    signer: 0,
                    commitment: vec![],
                })
                .unwrap();
        }

        assert!(coordinator.has_nonce_threshold());
    }
}
