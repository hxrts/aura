//! Coordinator role implementation
//!
//! This module contains methods for the coordinator role in consensus.

use super::{types::ProtocolStats, ConsensusProtocol};
use crate::{
    messages::{ConsensusMessage, ConsensusPhase},
    protocol::guards::{ConsensusResultGuard, SignRequestGuard},
    types::{consensus_signing_bytes, CommitFact},
    ConsensusId,
};
use aura_core::{
    crypto::tree_signing::frost_aggregate,
    effects::PhysicalTimeEffects,
    time::{PhysicalTime, ProvenancedTime, TimeStamp},
    AuraError, AuthorityId, Result,
};
use aura_guards::guards::traits::GuardContextProvider;
use aura_guards::GuardEffects;
use std::collections::BTreeMap;
use tracing::{debug, warn};

impl ConsensusProtocol {
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
        if !self.config.witness_set.contains(&sender) {
            return Err(AuraError::permission_denied(format!(
                "Authority {sender} is not in consensus witness set"
            )));
        }

        let mut instances = self.instances.write().await;

        let instance = instances
            .get_mut(&consensus_id)
            .ok_or_else(|| AuraError::invalid("Unknown consensus instance"))?;

        match message {
            ConsensusMessage::NonceCommit { commitment, .. } => {
                instance.tracker.add_nonce(sender, commitment)?;

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
                instance.tracker.add_conflict(sender, conflicts)?;
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
        let instances = self.instances.read().await;
        let instance = instances
            .get(&consensus_id)
            .ok_or_else(|| AuraError::internal("Instance not found"))?;

        let signatures = instance.tracker.get_signatures();
        let participants = instance.tracker.get_participants();
        let signing_ids: std::collections::BTreeSet<_> = signatures
            .iter()
            .map(|signature| signature.signer)
            .collect();

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
            if signing_ids.contains(&commitment.signer) {
                commitments.insert(commitment.signer, commitment.clone());
                debug!(witness = %witness, signer = %commitment.signer, "Using nonce commitment for aggregation");
            }
        }

        let signing_bytes = consensus_signing_bytes(
            consensus_id,
            instance.prestate_hash,
            instance.operation_hash,
            &instance.operation_bytes,
            self.config.epoch,
        )?;

        let aggregated_sig =
            frost_aggregate(&signatures, &signing_bytes, &commitments, &frost_group_pkg)
                .map_err(|e| AuraError::crypto(format!("FROST aggregation failed: {e}")))?;

        let threshold_signature = aura_core::frost::ThresholdSignature {
            signature: aggregated_sig,
            signers: signatures.iter().map(|s| s.signer).collect(),
        };

        let ts_ms = effects
            .physical_time()
            .await
            .map_err(|e| AuraError::internal(format!("Consensus timestamp unavailable: {e}")))?
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
            instance.prestate_hash,
            instance.operation_hash,
            instance.operation_bytes.clone(),
            self.config.epoch,
            threshold_signature,
            None, // Would include group public key
            participants,
            self.config.witnesses().to_vec(),
            self.config.threshold(),
            instance.phase == ConsensusPhase::Execute, // Fast path if we skipped nonce phase
            timestamp,
        );

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
    use crate::{evidence::EvidenceDelta, types::ConsensusConfig};
    use async_trait::async_trait;
    use aura_core::{
        effects::{
            authorization::AuthorizationError, storage::StorageError, time::TimeError,
            AuthorizationEffects, FlowBudgetEffects, JournalEffects, LeakageBudget, LeakageEffects,
            LeakageEvent, ObserverClass, RandomCoreEffects, StorageCoreEffects,
            StorageExtendedEffects,
        },
        frost::{PartialSignature, PublicKeyPackage},
        time::PhysicalTime,
        types::{
            flow::{FlowCost, FlowNonce, Receipt, ReceiptSig},
            scope::{AuthorizationOp, ResourceScope},
            Epoch,
        },
        AuraResult, Cap, ContextId, FlowBudget, Hash32, Journal,
    };
    use std::collections::HashMap;
    use std::result::Result as StdResult;

    struct NoopEffects {
        authority_id: AuthorityId,
    }

    impl GuardContextProvider for NoopEffects {
        fn authority_id(&self) -> AuthorityId {
            self.authority_id
        }

        fn get_metadata(&self, _key: &str) -> Option<String> {
            None
        }
    }

    #[async_trait]
    impl PhysicalTimeEffects for NoopEffects {
        async fn physical_time(&self) -> StdResult<PhysicalTime, TimeError> {
            Ok(PhysicalTime::exact(1_700_000_000_000))
        }

        async fn sleep_ms(&self, _ms: u64) -> StdResult<(), TimeError> {
            Ok(())
        }
    }

    #[async_trait]
    impl RandomCoreEffects for NoopEffects {
        async fn random_bytes(&self, len: usize) -> Vec<u8> {
            vec![0; len]
        }

        async fn random_bytes_32(&self) -> [u8; 32] {
            [0; 32]
        }

        async fn random_u64(&self) -> u64 {
            0
        }
    }

    #[async_trait]
    impl StorageCoreEffects for NoopEffects {
        async fn store(&self, _key: &str, _value: Vec<u8>) -> StdResult<(), StorageError> {
            Ok(())
        }

        async fn retrieve(&self, _key: &str) -> StdResult<Option<Vec<u8>>, StorageError> {
            Ok(None)
        }

        async fn remove(&self, _key: &str) -> StdResult<bool, StorageError> {
            Ok(false)
        }

        async fn list_keys(&self, _prefix: Option<&str>) -> StdResult<Vec<String>, StorageError> {
            Ok(Vec::new())
        }
    }

    impl StorageExtendedEffects for NoopEffects {}

    #[async_trait]
    impl JournalEffects for NoopEffects {
        async fn merge_facts(&self, target: Journal, _delta: Journal) -> Result<Journal> {
            Ok(target)
        }

        async fn refine_caps(&self, target: Journal, _refinement: Journal) -> Result<Journal> {
            Ok(target)
        }

        async fn get_journal(&self) -> Result<Journal> {
            Ok(Journal::new())
        }

        async fn persist_journal(&self, _journal: &Journal) -> Result<()> {
            Ok(())
        }

        async fn get_flow_budget(
            &self,
            _context: &ContextId,
            _peer: &AuthorityId,
        ) -> Result<FlowBudget> {
            Ok(FlowBudget::new(1_000, Epoch::from(1)))
        }

        async fn update_flow_budget(
            &self,
            _context: &ContextId,
            _peer: &AuthorityId,
            budget: &FlowBudget,
        ) -> Result<FlowBudget> {
            Ok(*budget)
        }

        async fn charge_flow_budget(
            &self,
            _context: &ContextId,
            _peer: &AuthorityId,
            _cost: FlowCost,
        ) -> Result<FlowBudget> {
            Ok(FlowBudget::new(1_000, Epoch::from(1)))
        }
    }

    #[async_trait]
    impl FlowBudgetEffects for NoopEffects {
        async fn charge_flow(
            &self,
            context: &ContextId,
            peer: &AuthorityId,
            cost: FlowCost,
        ) -> AuraResult<Receipt> {
            Ok(Receipt::new(
                *context,
                self.authority_id,
                *peer,
                Epoch::from(1),
                cost,
                FlowNonce::new(0),
                Hash32::default(),
                ReceiptSig::new(Vec::new())?,
            ))
        }
    }

    #[async_trait]
    impl AuthorizationEffects for NoopEffects {
        async fn verify_capability(
            &self,
            _capabilities: &Cap,
            _operation: AuthorizationOp,
            _scope: &ResourceScope,
        ) -> StdResult<bool, AuthorizationError> {
            Ok(true)
        }

        async fn delegate_capabilities(
            &self,
            source_capabilities: &Cap,
            _requested_capabilities: &Cap,
            _target_authority: &AuthorityId,
        ) -> StdResult<Cap, AuthorizationError> {
            Ok(source_capabilities.clone())
        }
    }

    #[async_trait]
    impl LeakageEffects for NoopEffects {
        async fn record_leakage(&self, _event: LeakageEvent) -> Result<()> {
            Ok(())
        }

        async fn get_leakage_budget(&self, _context_id: ContextId) -> Result<LeakageBudget> {
            Ok(LeakageBudget::zero())
        }

        async fn check_leakage_budget(
            &self,
            _context_id: ContextId,
            _observer: ObserverClass,
            _amount: u64,
        ) -> Result<bool> {
            Ok(true)
        }

        async fn get_leakage_history(
            &self,
            _context_id: ContextId,
            _since_timestamp: Option<&PhysicalTime>,
        ) -> Result<Vec<LeakageEvent>> {
            Ok(Vec::new())
        }
    }

    fn authority(seed: u8) -> AuthorityId {
        AuthorityId::new_from_entropy([seed; 32])
    }

    #[tokio::test]
    async fn coordinator_rejects_sign_share_from_non_witness() {
        let coordinator = authority(1);
        let witness = authority(2);
        let non_witness = authority(99);
        let context_id = ContextId::new_from_entropy([7; 32]);
        let config = ConsensusConfig::new(1, vec![witness], Epoch::from(1)).unwrap();
        let protocol = ConsensusProtocol::new(
            coordinator,
            context_id,
            config,
            HashMap::new(),
            PublicKeyPackage::new(Vec::new(), BTreeMap::default(), 1, 1),
        )
        .unwrap();
        let effects = NoopEffects {
            authority_id: coordinator,
        };
        let consensus_id = ConsensusId::new(Hash32::default(), Hash32([1; 32]), 7);
        let message = ConsensusMessage::SignShare {
            consensus_id,
            result_id: Hash32([2; 32]),
            share: PartialSignature {
                signer: 99,
                signature: vec![9; 32],
            },
            next_commitment: None,
            epoch: Epoch::from(1),
            evidence_delta: EvidenceDelta::empty(consensus_id, 1),
        };

        let err = protocol
            .process_coordinator_message(message, non_witness, &effects)
            .await
            .unwrap_err();

        assert!(matches!(err, AuraError::PermissionDenied { .. }));
    }
}
