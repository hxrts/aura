//! Witness Management for Consensus
//!
//! This module handles witness selection, shares, and threshold verification
//! for the Aura Consensus protocol.

use super::ConsensusId;
use aura_core::frost::{NonceCommitment, PartialSignature};
use aura_core::{AuthorityId, Hash32};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// A set of witnesses for a consensus instance
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WitnessSet {
    /// Required threshold for consensus
    pub threshold: u16,

    /// Selected witnesses by authority ID
    pub witnesses: Vec<AuthorityId>,

    /// Nonce commitments from witnesses
    pub nonce_commitments: BTreeMap<AuthorityId, NonceCommitment>,

    /// Collected shares from witnesses
    pub shares: BTreeMap<AuthorityId, WitnessShare>,
}

impl WitnessSet {
    /// Create a new witness set
    pub fn new(threshold: u16, witnesses: Vec<AuthorityId>) -> Self {
        Self {
            threshold,
            witnesses,
            nonce_commitments: BTreeMap::new(),
            shares: BTreeMap::new(),
        }
    }

    /// Check if witness is part of this set
    pub fn contains(&self, authority: &AuthorityId) -> bool {
        self.witnesses.contains(authority)
    }

    /// Add a nonce commitment from a witness
    pub fn add_nonce_commitment(
        &mut self,
        authority: AuthorityId,
        commitment: NonceCommitment,
    ) -> Result<(), String> {
        if !self.contains(&authority) {
            return Err("Authority not in witness set".to_string());
        }

        self.nonce_commitments.insert(authority, commitment);
        Ok(())
    }

    /// Add a witness share
    pub fn add_share(&mut self, authority: AuthorityId, share: WitnessShare) -> Result<(), String> {
        if !self.contains(&authority) {
            return Err("Authority not in witness set".to_string());
        }

        // Verify share matches nonce commitment
        if !self.nonce_commitments.contains_key(&authority) {
            return Err("Missing nonce commitment from authority".to_string());
        }

        self.shares.insert(authority, share);
        Ok(())
    }

    /// Check if we have enough shares for threshold
    pub fn has_threshold(&self) -> bool {
        self.shares.len() >= self.threshold as usize
    }

    /// Get participating authorities
    pub fn participants(&self) -> Vec<AuthorityId> {
        self.shares.keys().cloned().collect()
    }
}

/// A witness share for consensus
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WitnessShare {
    /// The consensus instance this share is for
    pub consensus_id: ConsensusId,

    /// The authority providing this share
    pub authority: AuthorityId,

    /// Partial signature from FROST
    pub partial_signature: PartialSignature,

    /// Hash of the operation being signed
    pub operation_hash: Hash32,

    /// Timestamp when share was created
    pub timestamp_ms: u64,
}

impl WitnessShare {
    /// Create a new witness share
    pub fn new(
        consensus_id: super::ConsensusId,
        authority: AuthorityId,
        partial_signature: PartialSignature,
        operation_hash: Hash32,
    ) -> Self {
        let timestamp_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;

        Self {
            consensus_id,
            authority,
            partial_signature,
            operation_hash,
            timestamp_ms,
        }
    }
}

/// Message types for witness communication
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WitnessMessage {
    /// Request to participate as witness
    ExecuteRequest {
        consensus_id: super::ConsensusId,
        prestate_hash: Hash32,
        operation_hash: Hash32,
        operation_bytes: Vec<u8>,
    },

    /// Nonce commitment from witness
    NonceCommitment {
        consensus_id: super::ConsensusId,
        authority: AuthorityId,
        commitment: NonceCommitment,
    },

    /// Partial signature share
    ShareResponse { share: WitnessShare },

    /// Gossip request for epidemic fallback
    GossipRequest {
        consensus_id: super::ConsensusId,
        prestate_hash: Hash32,
        operation_hash: Hash32,
        operation_bytes: Vec<u8>,
        requester: AuthorityId,
    },

    /// Conflict detected
    ConflictReport {
        consensus_id: super::ConsensusId,
        reporter: AuthorityId,
        conflicting_operations: Vec<Hash32>,
    },
}

/// Witness role in consensus protocol
pub struct WitnessRole {
    /// Our authority ID
    pub authority_id: AuthorityId,

    /// Active consensus instances we're witnessing
    pub active_instances: BTreeMap<super::ConsensusId, WitnessInstance>,
}

/// State for a single consensus instance as a witness
pub struct WitnessInstance {
    pub consensus_id: super::ConsensusId,
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
            active_instances: BTreeMap::new(),
        }
    }

    /// Handle an execute request
    pub async fn handle_execute_request(
        &mut self,
        consensus_id: super::ConsensusId,
        prestate_hash: Hash32,
        operation_hash: Hash32,
        operation_bytes: Vec<u8>,
    ) -> Result<WitnessMessage, String> {
        // TODO: Verify prestate matches our view
        // TODO: Generate nonce commitment using FROST

        let instance = WitnessInstance {
            consensus_id,
            prestate_hash,
            operation_hash,
            nonce_commitment: None, // TODO: Generate with FROST
            partial_signature: None,
        };

        self.active_instances.insert(consensus_id, instance);

        // For now, return a placeholder
        Ok(WitnessMessage::NonceCommitment {
            consensus_id,
            authority: self.authority_id,
            commitment: NonceCommitment {
                signer: 0,          // TODO: Real signer ID
                commitment: vec![], // TODO: Real FROST commitment
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_witness_set() {
        let authorities = vec![AuthorityId::new(), AuthorityId::new(), AuthorityId::new()];

        let mut witness_set = WitnessSet::new(2, authorities.clone());
        assert!(!witness_set.has_threshold());

        // Add shares
        for auth in &authorities[..2] {
            let share = WitnessShare::new(
                ConsensusId(Hash32::new([0; 32])),
                *auth,
                PartialSignature {
                    signer: 0,
                    signature: vec![],
                },
                Hash32::new([0; 32]),
            );

            // Should fail without nonce commitment
            assert!(witness_set.add_share(*auth, share.clone()).is_err());

            // Add nonce commitment first
            witness_set
                .add_nonce_commitment(
                    *auth,
                    NonceCommitment {
                        signer: 0,
                        commitment: vec![],
                    },
                )
                .unwrap();

            // Now share should succeed
            witness_set.add_share(*auth, share).unwrap();
        }

        assert!(witness_set.has_threshold());
        assert_eq!(witness_set.participants().len(), 2);
    }
}
