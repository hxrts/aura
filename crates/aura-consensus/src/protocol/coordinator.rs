//! Coordinator role implementation
//!
//! This module contains methods for the coordinator role in consensus.

use super::{types::ProtocolStats, ConsensusProtocol};
use crate::{
    messages::{ConsensusMessage, ConsensusPhase},
    types::CommitFact,
    ConsensusId,
};
use aura_core::{
    crypto::tree_signing::frost_aggregate,
    time::{PhysicalTime, ProvenancedTime, TimeStamp},
    AuraError, AuthorityId, Result,
};
use std::collections::BTreeMap;
use tracing::{debug, warn};

impl ConsensusProtocol {
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
                if instance
                    .tracker
                    .has_nonce_threshold(self.config.threshold())
                {
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
                result_id,
                share,
                next_commitment,
                epoch,
                ..
            } => {
                // Add signature with result_id tracking via ShareCollector
                match instance.tracker.add_signature(sender, share, result_id) {
                    Ok(Some(_threshold_set)) => {
                        // This result_id reached threshold - finalize consensus
                        debug!(sender = %sender, result_id = %result_id, "Threshold reached");

                        // Sync core state after adding share
                        // Quint: applyShare action / Lean: Consensus.Agreement
                        instance.sync_core_state();
                        instance.assert_invariants();

                        return self.finalize_consensus(consensus_id).await;
                    }
                    Ok(None) => {
                        // Share added, but threshold not yet reached
                        debug!(sender = %sender, result_id = %result_id, "Share added");

                        // Sync core state after adding share
                        instance.sync_core_state();
                        instance.assert_invariants();
                    }
                    Err(e) => {
                        // Duplicate or other error
                        warn!(sender = %sender, error = %e, "Failed to add signature");
                    }
                }

                // Cache next commitment if provided
                if let (Some(commitment), _) = (next_commitment, epoch == self.config.epoch) {
                    debug!(sender = %sender, "Cached pipelined commitment for next round");
                    // Would be handled by witness state manager
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
    pub(super) async fn finalize_consensus(
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
            .map_err(|e: AuraError| {
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

        let ts_ms = 0u64; // Would be set by time effects
        let timestamp = ProvenancedTime {
            stamp: TimeStamp::PhysicalClock(PhysicalTime {
                ts_ms,
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
            self.config.threshold(),
            instance.phase == ConsensusPhase::Execute, // Fast path if we skipped nonce phase
            timestamp,
        );

        // TODO: Attach actual evidence delta from tracker
        let evidence_delta = crate::evidence::EvidenceDelta::empty(commit_fact.consensus_id, ts_ms);

        Ok(Some(ConsensusMessage::ConsensusResult {
            commit_fact,
            evidence_delta,
        }))
    }

    /// Get protocol statistics
    pub async fn get_stats(&self) -> ProtocolStats {
        let instances = self.instances.read().await;

        ProtocolStats {
            active_instances: instances.len(),
            epoch: self.config.epoch,
            threshold: self.config.threshold(),
            witness_count: self.config.witness_set.len(),
        }
    }
}
