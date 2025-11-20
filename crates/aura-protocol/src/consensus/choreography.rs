//! Consensus Choreography
//!
//! This module implements the choreographic protocol for Aura Consensus,
//! integrating with FROST threshold signatures to produce consensus proofs.

use super::{CommitFact, ConsensusId, WitnessMessage, WitnessShare};
use aura_core::frost::{NonceCommitment, PartialSignature, ThresholdSignature};
use aura_core::{AuraError, AuthorityId, Hash32, Result};
use aura_macros::choreography;
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
        // TODO: Actually aggregate signatures using FROST
        // For now, create a placeholder
        let threshold_signature = ThresholdSignature {
            signature: vec![],
            signers: vec![],
        };

        let participants: Vec<_> = self.collected_shares.keys().cloned().collect();

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
    ) -> Result<ConsensusMessage> {
        // TODO: Verify prestate matches our view
        // TODO: Generate real nonce using FROST

        let nonce_commitment = NonceCommitment {
            signer: 0,
            commitment: vec![],
        }; // Placeholder

        let instance = WitnessInstance {
            prestate_hash,
            operation_hash,
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
        let partial_signature = PartialSignature {
            signer: 0,
            signature: vec![],
        }; // Placeholder

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
pub async fn run_consensus_choreography<E>(
    effect_handler: &E,
    config: ConsensusChoreographyConfig,
    operation: &[u8],
) -> Result<CommitFact>
where
    E: aura_core::effects::CryptoEffects + aura_core::effects::NetworkEffects,
{
    // Implementation of choreographic consensus protocol execution
    
    // Phase 1: Set up coordinator role
    let mut coordinator = CoordinatorRole::new(config.witnesses[0], config.clone());
    
    // Hash the operation for consensus
    let operation_hash = Hash32(aura_core::hash::hash(operation));
    let prestate_hash = Hash32::default(); // TODO: Get actual prestate hash
    
    // Phase 2: Execute choreography protocol
    match execute_consensus_phases(
        effect_handler, 
        &mut coordinator, 
        prestate_hash,
        operation_hash,
        operation.to_vec()
    ).await {
        Ok(commit_fact) => {
            // Phase 3: Verify and return result
            commit_fact
                .verify()
                .map_err(|e| AuraError::invalid(e))?;
            Ok(commit_fact)
        },
        Err(e) => {
            // Handle consensus failure
            eprintln!("Consensus choreography failed: {}", e);
            Err(e)
        }
    }
}

/// Execute the four-phase consensus choreography
async fn execute_consensus_phases<E>(
    effect_handler: &E,
    coordinator: &mut CoordinatorRole,
    prestate_hash: Hash32,
    operation_hash: Hash32,
    operation_bytes: Vec<u8>,
) -> Result<CommitFact>
where
    E: aura_core::effects::CryptoEffects + aura_core::effects::NetworkEffects,
{
    // Phase 1: Broadcast execute request to witnesses
    let execute_message = coordinator.create_execute_message(
        prestate_hash,
        operation_hash,
        operation_bytes.clone(),
    );
    
    broadcast_to_witnesses(effect_handler, &coordinator.config.witnesses, &execute_message).await?;
    
    // Phase 2: Collect nonce commitments  
    let nonce_timeout = std::time::Duration::from_millis(coordinator.config.timeout_ms / 3);
    collect_nonce_commitments(effect_handler, coordinator, nonce_timeout).await?;
    
    if !coordinator.has_nonce_threshold() {
        return Err(AuraError::Internal {
            message: "Insufficient nonce commitments for consensus".to_string(),
        });
    }
    
    // Phase 3: Request signatures with aggregated nonces
    let sign_request = coordinator.create_sign_request();
    broadcast_to_witnesses(effect_handler, &coordinator.config.witnesses, &sign_request).await?;
    
    // Phase 4: Collect partial signatures
    let signature_timeout = std::time::Duration::from_millis(coordinator.config.timeout_ms / 3);
    collect_signature_shares(effect_handler, coordinator, signature_timeout).await?;
    
    if !coordinator.has_signature_threshold() {
        return Err(AuraError::Internal {
            message: "Insufficient signature shares for consensus".to_string(),
        });
    }
    
    // Phase 5: Create and broadcast final result
    let commit_fact = coordinator.create_commit_fact(
        prestate_hash,
        operation_hash,
        operation_bytes,
    )?;
    
    let result_message = ConsensusMessage::ConsensusResult { 
        commit_fact: commit_fact.clone() 
    };
    broadcast_to_witnesses(effect_handler, &coordinator.config.witnesses, &result_message).await?;
    
    Ok(commit_fact)
}

/// Broadcast message to all witnesses
async fn broadcast_to_witnesses<E>(
    _effect_handler: &E,
    witnesses: &[AuthorityId],
    message: &ConsensusMessage,
) -> Result<()>
where
    E: aura_core::effects::NetworkEffects,
{
    // TODO: Use actual NetworkEffects to send messages
    // For now, simulate broadcasting
    let _serialized = serde_json::to_vec(message)
        .map_err(|e| AuraError::serialization(e.to_string()))?;
    
    // Simulate sending to each witness
    for _witness in witnesses {
        // TODO: effect_handler.send_to_peer(witness_uuid, serialized.clone()).await?;
    }
    
    Ok(())
}

/// Collect nonce commitments from witnesses
async fn collect_nonce_commitments<E>(
    _effect_handler: &E,
    coordinator: &mut CoordinatorRole,
    _timeout: std::time::Duration,
) -> Result<()>
where
    E: aura_core::effects::CryptoEffects + aura_core::effects::NetworkEffects,
{
    // TODO: Use actual NetworkEffects to receive messages
    // For now, simulate collecting commitments
    
    // Collect witnesses first to avoid borrowing issues
    let witnesses_to_commit: Vec<_> = coordinator
        .config
        .witnesses
        .iter()
        .take(coordinator.config.threshold as usize)
        .enumerate()
        .map(|(i, witness)| (i, *witness))
        .collect();
    
    // Simulate receiving nonce commitments from witnesses
    for (i, witness) in witnesses_to_commit {
        // Simulate a witness providing nonce commitment
        // TODO: Generate real FROST nonce commitment
        let commitment = NonceCommitment {
            signer: i as u16,
            commitment: vec![i as u8; 32], // Placeholder commitment
        };
        
        coordinator.handle_nonce_commit(witness, commitment)?;
    }
    
    Ok(())
}

/// Collect signature shares from witnesses  
async fn collect_signature_shares<E>(
    _effect_handler: &E,
    coordinator: &mut CoordinatorRole,
    _timeout: std::time::Duration,
) -> Result<()>
where
    E: aura_core::effects::CryptoEffects + aura_core::effects::NetworkEffects,
{
    // TODO: Use actual NetworkEffects to receive messages
    // For now, simulate collecting shares
    
    // Collect witnesses first to avoid borrowing issues
    let witnesses_to_sign: Vec<_> = coordinator
        .config
        .witnesses
        .iter()
        .take(coordinator.config.threshold as usize)
        .enumerate()
        .map(|(i, witness)| (i, *witness))
        .collect();
    
    // Simulate receiving signature shares from witnesses
    for (i, witness) in witnesses_to_sign {
        // Simulate a witness providing signature share
        // TODO: Generate real FROST partial signature
        let share = PartialSignature {
            signer: i as u16,
            signature: vec![i as u8; 64], // Placeholder signature
        };
        
        coordinator.handle_sign_share(witness, share)?;
    }
    
    Ok(())
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
