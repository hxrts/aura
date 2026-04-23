//! Coordinator role implementation
//!
//! This module contains methods for the coordinator role in consensus.

use super::{types::ProtocolStats, ConsensusProtocol};
use crate::{
    frost::verify_partial_signature,
    messages::{ConsensusMessage, ConsensusPhase},
    protocol::guards::{ConsensusResultGuard, SignRequestGuard},
    types::{consensus_commit_transcript_bytes, CommitFact},
    ConsensusId,
};
use aura_core::{
    crypto::tree_signing::frost_aggregate,
    effects::PhysicalTimeEffects,
    time::{PhysicalTime, ProvenancedTime, TimeStamp},
    AuraError, AuthorityId, Hash32, Result,
};
use aura_guards::guards::traits::GuardContextProvider;
use aura_guards::GuardEffects;
use std::collections::BTreeMap;
use tracing::{debug, warn};

impl ConsensusProtocol {
    fn validate_sign_share(
        &self,
        instance: &super::instance::ProtocolInstance,
        sender: AuthorityId,
        result_id: Hash32,
        share: &aura_core::frost::PartialSignature,
        epoch: aura_core::types::Epoch,
    ) -> Result<()> {
        if instance.phase != ConsensusPhase::Sign {
            return Err(AuraError::invalid(
                "SignShare received outside signing phase",
            ));
        }

        if !self.config.witnesses().contains(&sender) {
            return Err(AuraError::invalid(
                "SignShare sender is not an active witness",
            ));
        }

        if epoch != self.config.epoch {
            return Err(AuraError::invalid(
                "SignShare epoch does not match active epoch",
            ));
        }

        if result_id != instance.operation_hash {
            return Err(AuraError::invalid(
                "SignShare result_id does not match active result",
            ));
        }

        let expected_share = self.witness_key_packages.get(&sender).ok_or_else(|| {
            AuraError::invalid("Missing witness key package for SignShare sender")
        })?;
        if share.signer != expected_share.identifier {
            return Err(AuraError::invalid(
                "SignShare signer does not match configured witness share",
            ));
        }

        let nonce_commitment = instance
            .tracker
            .nonce_commitments
            .get(&sender)
            .ok_or_else(|| AuraError::invalid("SignShare missing nonce commitment for sender"))?;
        if nonce_commitment.signer != share.signer {
            return Err(AuraError::invalid(
                "SignShare signer does not match committed nonce signer",
            ));
        }

        if share.signature.is_empty() {
            return Err(AuraError::invalid("SignShare payload is empty"));
        }

        let transcript = consensus_commit_transcript_bytes(
            instance.consensus_id,
            instance.prestate_hash,
            instance.operation_hash,
            &instance.operation_bytes,
            self.config.threshold(),
        )?;
        let aggregated_nonces = instance.tracker.get_nonces();
        verify_partial_signature(
            share,
            &transcript,
            &aggregated_nonces,
            &self.group_public_key,
        )?;

        Ok(())
    }

    /// Process incoming message (coordinator role)
    ///
    /// Guards are evaluated before constructing response messages to enforce:
    /// - Authorization requirements (via CapGuard)
    /// - Flow budget constraints (via FlowGuard)
    /// - Privacy budgets (via LeakageTracker)
    pub async fn process_coordinator_message<E>(
        &self,
        message: ConsensusMessage,
        sender: AuthorityId,
        effects: &E,
    ) -> Result<Option<ConsensusMessage>>
    where
        E: GuardEffects + GuardContextProvider + PhysicalTimeEffects,
    {
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

                    // Evaluate guards for all witnesses before broadcasting SignRequest
                    // We check one witness as a representative (they all get the same message)
                    if let Some(first_witness) = self.config.witnesses().first() {
                        let guard = SignRequestGuard::new(self.context_id, *first_witness);
                        let guard_result = guard.evaluate(effects).await?;
                        self.require_send_guard_authorized(
                            consensus_id,
                            "SignRequest",
                            "Guard denied SignRequest",
                            guard_result,
                        )?;
                    }

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
                if let Err(error) =
                    self.validate_sign_share(instance, sender, result_id, &share, epoch)
                {
                    warn!(sender = %sender, error = %error, "Rejected invalid SignShare");
                    return Ok(None);
                }

                // Add signature with result_id tracking via ShareCollector
                match instance.tracker.add_signature(sender, share, result_id) {
                    Ok(Some(_threshold_set)) => {
                        // This result_id reached threshold - finalize consensus
                        debug!(sender = %sender, result_id = %result_id, "Threshold reached");

                        // Sync core state after adding share
                        // Quint: applyShare action / Lean: Consensus.Agreement
                        instance.sync_core_state();
                        instance.assert_invariants();

                        return self.finalize_consensus(consensus_id, effects).await;
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
    ///
    /// # Journal Coupling & Charge-Before-Send
    ///
    /// This method creates the CommitFact but does NOT commit it to the journal.
    /// The caller (runtime bridge) is responsible for:
    /// 1. Committing the CommitFact via `commit_relational_facts()`
    /// 2. Broadcasting the ConsensusResult message via transport
    ///
    /// This ensures the charge-before-send invariant at the runtime bridge layer
    /// where both journal and transport effects are available.
    pub(super) async fn finalize_consensus<E>(
        &self,
        consensus_id: ConsensusId,
        effects: &E,
    ) -> Result<Option<ConsensusMessage>>
    where
        E: GuardEffects + GuardContextProvider + PhysicalTimeEffects,
    {
        let (
            prestate_hash,
            operation_hash,
            operation_bytes,
            fast_path,
            nonce_commitments,
            threshold_result,
            signatures,
            participants,
        ) = {
            let instances = self.instances.read().await;
            let instance = instances
                .get(&consensus_id)
                .ok_or_else(|| AuraError::internal("Instance not found"))?;
            let threshold_result = instance
                .tracker
                .get_threshold_result()
                .ok_or_else(|| AuraError::invalid("No threshold result available"))?;

            (
                instance.prestate_hash,
                instance.operation_hash,
                instance.operation_bytes.clone(),
                instance.phase == ConsensusPhase::Execute,
                instance.tracker.nonce_commitments.clone(),
                threshold_result,
                instance
                    .tracker
                    .get_signatures_for_result(&threshold_result),
                instance
                    .tracker
                    .get_participants_for_result(&threshold_result),
            )
        };

        if threshold_result != operation_hash {
            return Err(AuraError::invalid(
                "Consensus threshold result does not match active operation hash",
            ));
        }

        // Aggregate using FROST
        let frost_group_pkg: frost_ed25519::keys::PublicKeyPackage = self
            .group_public_key
            .clone()
            .try_into()
            .map_err(|e: AuraError| {
                AuraError::crypto(format!("Invalid group public key package: {e}"))
            })?;

        let mut commitments = BTreeMap::new();
        for (witness, commitment) in &nonce_commitments {
            commitments.insert(commitment.signer, commitment.clone());
            debug!(witness = %witness, signer = %commitment.signer, "Using nonce commitment for aggregation");
        }

        let transcript = consensus_commit_transcript_bytes(
            consensus_id,
            prestate_hash,
            operation_hash,
            &operation_bytes,
            self.config.threshold(),
        )?;
        let aggregated_sig =
            frost_aggregate(&signatures, &transcript, &commitments, &frost_group_pkg)
                .map_err(|e| AuraError::crypto(format!("FROST aggregation failed: {e}")))?;

        let threshold_signature = aura_core::frost::ThresholdSignature {
            signature: aggregated_sig,
            signers: signatures.iter().map(|s| s.signer).collect(),
        };

        let ts_ms = effects
            .physical_time()
            .await
            .map_err(|e| AuraError::internal(format!("time error: {e}")))?
            .ts_ms;
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
            prestate_hash,
            operation_hash,
            operation_bytes,
            threshold_signature,
            Some(self.group_public_key.clone()),
            participants,
            self.config.threshold(),
            fast_path,
            timestamp,
        );
        commit_fact.verify()?;

        // Get evidence delta from tracker
        let evidence_delta = self
            .evidence_tracker
            .write()
            .await
            .get_delta(commit_fact.consensus_id, ts_ms);

        // Evaluate guards before broadcasting ConsensusResult
        // We check one witness as a representative (they all get the same message)
        if let Some(first_witness) = self.config.witnesses().first() {
            let guard = ConsensusResultGuard::new(self.context_id, *first_witness);
            let guard_result = guard.evaluate(effects).await?;
            self.require_send_guard_authorized(
                consensus_id,
                "ConsensusResult",
                "Guard denied ConsensusResult",
                guard_result,
            )?;
        }

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        core::{state::ConsensusThreshold, ConsensusState as CoreState, PathSelection},
        protocol::instance::{ProtocolInstance, ProtocolRole},
        witness::{WitnessSet, WitnessTracker},
    };
    use aura_core::{
        crypto::tree_signing::NonceToken,
        frost::{NonceCommitment, PartialSignature, PublicKeyPackage, Share},
        types::Epoch,
        ContextId, Hash32, OperationId,
    };
    use rand::SeedableRng;
    use rand_chacha::ChaCha20Rng;
    use std::collections::{BTreeSet, HashMap};

    fn authority(seed: u8) -> AuthorityId {
        AuthorityId::new_from_entropy([seed; 32])
    }

    struct SignShareFixture {
        protocol: ConsensusProtocol,
        sender: AuthorityId,
        result_id: Hash32,
        instance: ProtocolInstance,
        valid_share: PartialSignature,
    }

    fn protocol_fixture() -> (ConsensusProtocol, AuthorityId, AuthorityId, Share, Share) {
        let witness_a = authority(1);
        let witness_b = authority(2);
        let config =
            crate::types::ConsensusConfig::new(2, vec![witness_a, witness_b], Epoch::from(7))
                .expect("config should build");
        let mut rng = ChaCha20Rng::from_seed([41u8; 32]);
        let (secret_shares, group_public_key) = frost_ed25519::keys::generate_with_dealer(
            2,
            2,
            frost_ed25519::keys::IdentifierList::Default,
            &mut rng,
        )
        .expect("dealer key generation should succeed");
        let mut share_values = secret_shares
            .into_iter()
            .map(|(_, secret_share)| {
                let key_package = frost_ed25519::keys::KeyPackage::try_from(secret_share)
                    .expect("secret share should convert to key package");
                Share::from(key_package)
            })
            .collect::<Vec<_>>();
        share_values.sort_by_key(|share| share.identifier);
        let share_a = share_values[0].clone();
        let share_b = share_values[1].clone();
        let mut key_packages = HashMap::new();
        key_packages.insert(witness_a, share_a.clone());
        key_packages.insert(witness_b, share_b.clone());
        let protocol = ConsensusProtocol::new(
            authority(9),
            ContextId::new_from_entropy([8; 32]),
            config,
            key_packages,
            PublicKeyPackage::from(group_public_key),
        )
        .expect("protocol should build");

        (protocol, witness_a, witness_b, share_a, share_b)
    }

    fn signing_instance(
        result_id: Hash32,
        commitments: [(AuthorityId, NonceCommitment); 2],
    ) -> ProtocolInstance {
        let witnesses: BTreeSet<_> = [authority(1), authority(2)].into_iter().collect();
        let mut tracker = WitnessTracker::with_threshold(2);
        for (witness, commitment) in commitments {
            tracker.add_nonce(witness, commitment);
        }
        let threshold = ConsensusThreshold::new(2).expect("threshold should build");
        let core_state = CoreState::new(
            ConsensusId(Hash32::from([3u8; 32])),
            OperationId::new_from_entropy(result_id.0),
            Hash32::from([4u8; 32]),
            threshold,
            witnesses,
            authority(9),
            PathSelection::FastPath,
        );

        ProtocolInstance {
            consensus_id: ConsensusId(Hash32::from([3u8; 32])),
            prestate_hash: Hash32::from([4u8; 32]),
            operation_hash: result_id,
            operation_bytes: vec![1, 2, 3],
            role: ProtocolRole::Coordinator {
                witness_set: WitnessSet::new(2, vec![authority(1), authority(2)])
                    .expect("witness set should build"),
            },
            tracker,
            phase: ConsensusPhase::Sign,
            start_time_ms: 0,
            nonce_token: None,
            core_state,
        }
    }

    fn signing_fixture() -> SignShareFixture {
        let (protocol, sender, peer, sender_share, peer_share) = protocol_fixture();
        let mut rng = ChaCha20Rng::from_seed([99u8; 32]);
        let result_id = Hash32::from([5u8; 32]);
        let transcript = consensus_commit_transcript_bytes(
            ConsensusId(Hash32::from([3u8; 32])),
            Hash32::from([4u8; 32]),
            result_id,
            &[1, 2, 3],
            2,
        )
        .expect("transcript should build");

        let sender_signing_share = sender_share.to_frost().expect("sender share should decode");
        let sender_nonces =
            frost_ed25519::round1::SigningNonces::new(&sender_signing_share, &mut rng);
        let sender_commitment = NonceCommitment {
            signer: sender_share.identifier,
            commitment: sender_nonces
                .commitments()
                .serialize()
                .expect("sender commitment should serialize"),
        };

        let peer_signing_share = peer_share.to_frost().expect("peer share should decode");
        let peer_nonces = frost_ed25519::round1::SigningNonces::new(&peer_signing_share, &mut rng);
        let peer_commitment = NonceCommitment {
            signer: peer_share.identifier,
            commitment: peer_nonces
                .commitments()
                .serialize()
                .expect("peer commitment should serialize"),
        };

        let aggregated_nonces = vec![sender_commitment.clone(), peer_commitment.clone()];
        let valid_share = protocol
            .frost_orchestrator
            .sign_with_nonce(
                &transcript,
                &sender_share,
                &NonceToken::from(sender_nonces),
                &aggregated_nonces,
            )
            .expect("partial signature should be created");
        let instance = signing_instance(
            result_id,
            [(sender, sender_commitment), (peer, peer_commitment)],
        );

        SignShareFixture {
            protocol,
            sender,
            result_id,
            instance,
            valid_share,
        }
    }

    #[test]
    fn validate_sign_share_accepts_bound_witness_share() {
        let fixture = signing_fixture();

        let result = fixture.protocol.validate_sign_share(
            &fixture.instance,
            fixture.sender,
            fixture.result_id,
            &fixture.valid_share,
            Epoch::from(7),
        );

        assert!(result.is_ok());
    }

    #[test]
    fn validate_sign_share_rejects_non_witness_sender() {
        let fixture = signing_fixture();

        let error = fixture
            .protocol
            .validate_sign_share(
                &fixture.instance,
                authority(42),
                fixture.result_id,
                &fixture.valid_share,
                Epoch::from(7),
            )
            .expect_err("non-witness sender should be rejected");

        assert!(error.to_string().contains("not an active witness"));
    }

    #[test]
    fn validate_sign_share_rejects_wrong_epoch_result_and_signer_binding() {
        let fixture = signing_fixture();

        let wrong_epoch = fixture
            .protocol
            .validate_sign_share(
                &fixture.instance,
                fixture.sender,
                fixture.result_id,
                &fixture.valid_share,
                Epoch::from(99),
            )
            .expect_err("wrong epoch should be rejected");
        assert!(wrong_epoch.to_string().contains("epoch"));

        let wrong_result = fixture
            .protocol
            .validate_sign_share(
                &fixture.instance,
                fixture.sender,
                Hash32::from([6u8; 32]),
                &fixture.valid_share,
                Epoch::from(7),
            )
            .expect_err("wrong result should be rejected");
        assert!(wrong_result.to_string().contains("result_id"));

        let mut wrong_signer_share = fixture.valid_share.clone();
        wrong_signer_share.signer = 2;

        let wrong_signer = fixture
            .protocol
            .validate_sign_share(
                &fixture.instance,
                fixture.sender,
                fixture.result_id,
                &wrong_signer_share,
                Epoch::from(7),
            )
            .expect_err("wrong signer binding should be rejected");
        assert!(wrong_signer
            .to_string()
            .contains("configured witness share"));
    }

    #[test]
    fn validate_sign_share_rejects_forged_partial_signature() {
        let fixture = signing_fixture();
        let mut forged_share = fixture.valid_share.clone();
        forged_share.signature = vec![0u8; 32];

        let error = fixture
            .protocol
            .validate_sign_share(
                &fixture.instance,
                fixture.sender,
                fixture.result_id,
                &forged_share,
                Epoch::from(7),
            )
            .expect_err("forged partial signature should be rejected");

        assert!(error
            .to_string()
            .contains("Partial signature verification failed"));
    }
}
