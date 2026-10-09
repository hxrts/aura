//! Witness role implementation
//!
//! This module contains methods for the witness role in consensus.

use super::{
    guards::{NonceCommitGuard, SignShareGuard},
    instance::{ProtocolInstance, ProtocolRole},
    ConsensusProtocol,
};
use crate::{
    core::{ConsensusState as CoreState, PathSelection},
    messages::{ConsensusMessage, ConsensusPhase},
    types::consensus_commit_transcript_bytes,
    witness::WitnessTracker,
    ConsensusId,
};
use aura_core::{
    effects::{PhysicalTimeEffects, RandomEffects},
    frost::{NonceCommitment, Share},
    AuraError, AuthorityId, OperationId, Result,
};
use aura_guards::guards::traits::GuardContextProvider;
use aura_guards::GuardEffects;
use std::collections::BTreeSet;
use tracing::info;

impl ConsensusProtocol {
    /// Participate as witness in consensus
    pub async fn participate_as_witness<E>(
        &self,
        message: ConsensusMessage,
        coordinator: AuthorityId,
        my_share: Share,
        random: &(impl RandomEffects + ?Sized),
        time: &(impl PhysicalTimeEffects + ?Sized),
        effects: &E,
    ) -> Result<Option<ConsensusMessage>>
    where
        E: GuardEffects + GuardContextProvider + PhysicalTimeEffects,
    {
        // Best-effort cleanup of stale instances before handling messages.
        if let Ok(now) = time.physical_time().await {
            let _ = self.cleanup_stale_instances(now.ts_ms).await;
        }

        // Merge incoming evidence delta before processing message
        let evidence_delta = message.evidence_delta().cloned();

        if let Some(delta) = evidence_delta {
            if let Ok(new_proofs) = self.evidence_tracker.write().await.merge(delta) {
                if new_proofs > 0 {
                    tracing::debug!("Merged {} new equivocation proofs", new_proofs);
                }
            }
        }

        match message {
            ConsensusMessage::Execute {
                consensus_id,
                prestate_hash,
                operation_hash,
                operation_bytes,
                ..
            } => {
                let threshold =
                    crate::core::state::ConsensusThreshold::new(self.config.threshold())
                        .ok_or_else(|| AuraError::invalid("Consensus threshold must be >= 1"))?;
                let witnesses: BTreeSet<_> = self.config.witness_set.iter().copied().collect();
                let operation_id = OperationId::new_from_entropy(operation_hash.0);

                // Initialize pure core state for invariant validation
                // Quint: startConsensus action / Lean: Consensus.Agreement
                let core_state = CoreState::new(
                    consensus_id,
                    operation_id,
                    prestate_hash,
                    threshold,
                    witnesses,
                    coordinator,
                    PathSelection::FastPath,
                );

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
                self.generate_nonce_commitment(
                    consensus_id,
                    coordinator,
                    &my_share,
                    random,
                    effects,
                )
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
                    coordinator,
                    aggregated_nonces,
                    &my_share,
                    random,
                    time,
                    effects,
                )
                .await
            }

            ConsensusMessage::ConsensusResult { commit_fact, .. } => {
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
    pub(super) async fn generate_nonce_commitment<E>(
        &self,
        consensus_id: ConsensusId,
        coordinator: AuthorityId,
        share: &Share,
        random: &(impl RandomEffects + ?Sized),
        effects: &E,
    ) -> Result<Option<ConsensusMessage>>
    where
        E: GuardEffects + GuardContextProvider + PhysicalTimeEffects,
    {
        let nonces = crate::frost::witness_nonce(share, random).await?;
        let commitment = nonces.commitment().clone();

        // Hold the nonces for signing when SignRequest arrives
        if let Some(instance) = self.instances.write().await.get_mut(&consensus_id) {
            instance.nonce_token = Some(nonces);
        }

        // Evaluate guards before sending NonceCommit to coordinator
        let guard = NonceCommitGuard::new(self.context_id, coordinator);
        let guard_result = guard.evaluate(effects).await?;
        self.require_send_guard_authorized(
            consensus_id,
            "NonceCommit",
            "Guard denied NonceCommit",
            guard_result,
        )?;

        Ok(Some(ConsensusMessage::NonceCommit {
            consensus_id,
            commitment,
        }))
    }

    /// Generate signature response (witness role)
    pub(super) async fn generate_signature_response<E>(
        &self,
        consensus_id: ConsensusId,
        coordinator: AuthorityId,
        aggregated_nonces: Vec<NonceCommitment>,
        share: &Share,
        random: &(impl RandomEffects + ?Sized),
        time: &(impl PhysicalTimeEffects + ?Sized),
        effects: &E,
    ) -> Result<Option<ConsensusMessage>>
    where
        E: GuardEffects + GuardContextProvider + PhysicalTimeEffects,
    {
        // Retrieve cached nonce token (slow path) or generate a fresh one if missing
        let mut instances = self.instances.write().await;
        let instance = instances
            .get_mut(&consensus_id)
            .ok_or_else(|| AuraError::invalid("Unknown consensus instance"))?;

        // The nonces committed for this round sign once. Without them there is
        // no commitment in the aggregated set to sign under.
        let nonces = instance
            .nonce_token
            .take()
            .ok_or_else(|| AuraError::invalid("no committed nonces for this consensus round"))?;

        let transcript = consensus_commit_transcript_bytes(
            consensus_id,
            instance.prestate_hash,
            instance.operation_hash,
            &instance.operation_bytes,
            self.config.threshold(),
        )?;

        // Sign using FROST with provided aggregated nonces
        let signature = self
            .frost_orchestrator
            .sign_with_nonce(&transcript, share, nonces, &aggregated_nonces)
            .await?;

        // Compute result_id from operation
        // For deterministic execution, result_id = operation_hash.
        // For deterministic execution, all honest witnesses get same result: result_id = operation_hash
        let result_id = instance.operation_hash;

        // Pipelined commitments (fast path nonce caching) are disabled until the interpreter
        // path supports proper capability token handoff. The choreography includes
        // leak="pipelined_commitment" annotation, but enforcement requires:
        // 1. Pure interpreter that returns capability tokens
        // 2. Explicit flow token handoff between rounds
        // 3. LeakageTracker integration with interpreter results
        // Until then, witnesses use slow path (generate nonce per round).
        let next_commitment = None;

        // Get evidence delta from tracker (with current timestamp)
        let ts_ms = time.physical_time().await.map(|t| t.ts_ms).unwrap_or(0);
        let evidence_delta = self
            .evidence_tracker
            .write()
            .await
            .get_delta(consensus_id, ts_ms);

        // Evaluate guards before sending SignShare to coordinator
        let guard = SignShareGuard::new(self.context_id, coordinator);
        let guard_result = guard.evaluate(effects).await?;
        self.require_send_guard_authorized(
            consensus_id,
            "SignShare",
            "Guard denied SignShare",
            guard_result,
        )?;

        Ok(Some(ConsensusMessage::SignShare {
            consensus_id,
            result_id,
            share: signature,
            next_commitment,
            epoch: self.config.epoch,
            evidence_delta,
        }))
    }
}
