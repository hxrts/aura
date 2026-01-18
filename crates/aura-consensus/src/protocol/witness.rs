//! Witness role implementation
//!
//! This module contains methods for the witness role in consensus.

use super::{
    instance::{ProtocolInstance, ProtocolRole},
    guards::{NonceCommitGuard, SignShareGuard},
    ConsensusProtocol,
};
use crate::{
    core::{ConsensusState as CoreState, PathSelection},
    messages::{ConsensusMessage, ConsensusPhase},
    witness::WitnessTracker,
    ConsensusId,
};
use aura_core::{
    crypto::tree_signing::NonceToken,
    effects::{PhysicalTimeEffects, RandomEffects},
    frost::{NonceCommitment, Share},
    AuraError, AuthorityId, OperationId, Result,
};
use aura_guards::guards::traits::GuardContextProvider;
use aura_guards::GuardEffects;
use frost_ed25519;
use rand::SeedableRng;
use std::collections::BTreeSet;
use tracing::{debug, info, warn};

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
        let evidence_delta = match &message {
            ConsensusMessage::Execute { evidence_delta, .. } => Some(evidence_delta.clone()),
            ConsensusMessage::SignShare { evidence_delta, .. } => Some(evidence_delta.clone()),
            ConsensusMessage::ConsensusResult { evidence_delta, .. } => {
                Some(evidence_delta.clone())
            }
            ConsensusMessage::Conflict { evidence_delta, .. } => Some(evidence_delta.clone()),
            _ => None,
        };

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
                cached_commitments: _,
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
                self.generate_nonce_commitment(consensus_id, coordinator, &my_share, random, effects)
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
                    &instance.operation_bytes,
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
        // Generate FROST nonces and commitment for this witness
        let seed = random.random_bytes_32().await;
        let mut rng = rand::rngs::StdRng::from_seed(seed);

        let signing_share = frost_ed25519::keys::SigningShare::deserialize(
            share
                .value
                .clone()
                .try_into()
                .map_err(|_| AuraError::crypto("Invalid signing share length"))?,
        )
        .map_err(|e| AuraError::crypto(format!("Invalid signing share: {e}")))?;

        let nonces = frost_ed25519::round1::SigningNonces::new(&signing_share, &mut rng);
        let commitment = NonceCommitment {
            signer: share.identifier,
            commitment: nonces
                .commitments()
                .serialize()
                .map_err(|e| AuraError::crypto(format!("Failed to serialize commitments: {e}")))?,
        };

        // Cache nonce token for signing when SignRequest arrives
        if let Some(instance) = self.instances.write().await.get_mut(&consensus_id) {
            instance.nonce_token = Some(NonceToken::from(nonces));
        }

        // Evaluate guards before sending NonceCommit to coordinator
        let guard = NonceCommitGuard::new(self.context_id, coordinator);
        let guard_result = guard.evaluate(effects).await?;

        if !guard_result.authorized {
            warn!(
                consensus_id = %consensus_id,
                reason = ?guard_result.denial_reason,
                "NonceCommit guard denied"
            );
            return Err(AuraError::permission_denied(
                guard_result
                    .denial_reason
                    .unwrap_or_else(|| "Guard denied NonceCommit".to_string()),
            ));
        }

        debug!(
            consensus_id = %consensus_id,
            receipt = ?guard_result.receipt.as_ref().map(|r| r.nonce),
            "NonceCommit guard authorized"
        );

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
        message: &[u8],
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

        let nonce_token = if let Some(token) = instance.nonce_token.take() {
            token
        } else {
            // Fallback: generate a fresh nonce and append its commitment
            let seed = random.random_bytes_32().await;
            let mut rng = rand::rngs::StdRng::from_seed(seed);
            let signing_share = frost_ed25519::keys::SigningShare::deserialize(
                share
                    .value
                    .clone()
                    .try_into()
                    .map_err(|_| AuraError::crypto("Invalid signing share length"))?,
            )
            .map_err(|e| AuraError::crypto(format!("Invalid signing share: {e}")))?;
            let nonces = frost_ed25519::round1::SigningNonces::new(&signing_share, &mut rng);
            let commitment = NonceCommitment {
                signer: share.identifier,
                commitment: nonces.commitments().serialize().map_err(|e| {
                    AuraError::crypto(format!("Failed to serialize commitments: {e}"))
                })?,
            };
            instance.tracker.add_nonce(self.authority_id, commitment);
            NonceToken::from(nonces)
        };

        // Sign using FROST with provided aggregated nonces
        let signature = self.frost_orchestrator.sign_with_nonce(
            message,
            share,
            &nonce_token,
            &aggregated_nonces,
        )?;

        // Compute result_id from operation
        // In current implementation, operation_bytes are signed directly (no execution step)
        // For deterministic execution, all honest witnesses get same result: result_id = operation_hash
        let result_id = instance.operation_hash;

        // TODO: No pipelined commitment until interpreter path supports token handoff
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

        if !guard_result.authorized {
            warn!(
                consensus_id = %consensus_id,
                reason = ?guard_result.denial_reason,
                "SignShare guard denied"
            );
            return Err(AuraError::permission_denied(
                guard_result
                    .denial_reason
                    .unwrap_or_else(|| "Guard denied SignShare".to_string()),
            ));
        }

        debug!(
            consensus_id = %consensus_id,
            receipt = ?guard_result.receipt.as_ref().map(|r| r.nonce),
            "SignShare guard authorized"
        );

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
