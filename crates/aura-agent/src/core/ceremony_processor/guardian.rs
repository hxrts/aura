//! Guardian Acceptance Handler
//!
//! Processes `application/aura-guardian-acceptance` envelopes.
//! Guardian acceptance indicates a guardian has agreed to participate
//! in a threshold ceremony (setup or rotation).

use super::ProcessResult;
use crate::runtime::effects::AuraEffectSystem;
use crate::runtime::services::ceremony_runner::{CeremonyCommitMetadata, CeremonyRunner};
use crate::runtime::services::CeremonyTracker;
use crate::ThresholdSigningService;
use aura_core::effects::transport::TransportEnvelope;
use aura_core::identifiers::CeremonyId;
use aura_core::threshold::{policy_for, CeremonyFlow, KeyGenerationPolicy};
use aura_core::{hash, AuthorityId, ContextId};
use aura_journal::fact::RelationalFact;
use aura_journal::ProtocolRelationalFact;

/// Handles guardian acceptance messages
pub struct GuardianHandler<'a> {
    authority_id: AuthorityId,
    effects: &'a AuraEffectSystem,
    ceremony_tracker: &'a CeremonyTracker,
    ceremony_runner: &'a CeremonyRunner,
    signing_service: &'a ThresholdSigningService,
}

impl<'a> GuardianHandler<'a> {
    /// Create a new guardian handler
    pub fn new(
        authority_id: AuthorityId,
        effects: &'a AuraEffectSystem,
        ceremony_tracker: &'a CeremonyTracker,
        ceremony_runner: &'a CeremonyRunner,
        signing_service: &'a ThresholdSigningService,
    ) -> Self {
        Self {
            authority_id,
            effects,
            ceremony_tracker,
            ceremony_runner,
            signing_service,
        }
    }

    /// Handle a guardian acceptance envelope
    pub async fn handle(&self, envelope: &TransportEnvelope) -> ProcessResult {
        let (Some(ceremony_id), Some(guardian_id)) = (
            envelope.metadata.get("ceremony-id"),
            envelope.metadata.get("guardian-id"),
        ) else {
            return ProcessResult::Skip;
        };
        let ceremony_id = CeremonyId::new(ceremony_id.clone());

        let guardian_authority: AuthorityId = match guardian_id.parse() {
            Ok(id) => id,
            Err(e) => {
                tracing::warn!(
                    ceremony_id = %ceremony_id,
                    guardian_id = %guardian_id,
                    error = %e,
                    "Invalid guardian authority id in acceptance"
                );
                let _ = self
                    .ceremony_runner
                    .abort(
                        &ceremony_id,
                        Some(format!("Invalid guardian id in acceptance: {guardian_id}")),
                    )
                    .await;
                return ProcessResult::Skip;
            }
        };

        let threshold_reached = match self
            .ceremony_runner
            .record_response(
                &ceremony_id,
                aura_core::threshold::ParticipantIdentity::guardian(guardian_authority),
            )
            .await
        {
            Ok(reached) => reached,
                Err(e) => {
                    tracing::warn!(
                        ceremony_id = %ceremony_id,
                        guardian_id = %guardian_id,
                        error = %e,
                        "Failed to mark guardian as accepted"
                    );
                    return ProcessResult::Skip;
                }
            };

        if !threshold_reached {
            return ProcessResult::Processed;
        }

        // Threshold reached - attempt to commit
        match self.commit_ceremony(&ceremony_id).await {
            Ok(()) => ProcessResult::Committed,
            Err(_) => ProcessResult::Processed,
        }
    }

    /// Commit a guardian ceremony after threshold is reached
    async fn commit_ceremony(&self, ceremony_id: &CeremonyId) -> Result<(), ()> {
        use aura_core::effects::ThresholdSigningEffects;

        let ceremony_state = match self.ceremony_tracker.get(ceremony_id).await {
            Ok(s) => s,
            Err(e) => {
                tracing::error!(
                    ceremony_id = %ceremony_id,
                    error = %e,
                    "Failed to retrieve ceremony state for commit"
                );
                return Err(());
            }
        };

        if ceremony_state.is_committed {
            return Ok(());
        }

        let new_epoch = ceremony_state.new_epoch;
        let policy = policy_for(CeremonyFlow::GuardianSetupRotation);
        let consensus_required = self
            .signing_service
            .threshold_state(&self.authority_id)
            .await
            .map(|state| state.threshold > 1 || state.total_participants > 1)
            .unwrap_or(true);

        // Check for consensus DKG transcript if required
        if policy.keygen == KeyGenerationPolicy::K3ConsensusDkg && consensus_required {
            let context_id = ContextId::new_from_entropy(hash::hash(&self.authority_id.to_bytes()));
            match self
                .effects
                .has_dkg_transcript_commit(self.authority_id, context_id, new_epoch)
                .await
            {
                Ok(true) => {}
                Ok(false) => {
                    let _ = self
                        .ceremony_runner
                        .abort(
                            ceremony_id,
                            Some("Missing consensus DKG transcript".to_string()),
                        )
                        .await;
                    return Err(());
                }
                Err(e) => {
                    tracing::error!(
                        ceremony_id = %ceremony_id,
                        error = %e,
                        "Failed to verify DKG transcript commit"
                    );
                    let _ = self
                        .ceremony_runner
                        .abort(ceremony_id, Some(format!("Transcript check failed: {e}")))
                        .await;
                    return Err(());
                }
            }
        } else if policy.keygen == KeyGenerationPolicy::K3ConsensusDkg && !consensus_required {
            tracing::info!(
                ceremony_id = %ceremony_id,
                "Skipping consensus transcript check (single-signer authority)"
            );
        }

        // Commit key rotation
        if let Err(e) = self
            .effects
            .commit_key_rotation(&self.authority_id, new_epoch)
            .await
        {
            tracing::error!(
                ceremony_id = %ceremony_id,
                new_epoch,
                error = %e,
                "Failed to commit guardian key rotation"
            );
            let _ = self
                .ceremony_runner
                .abort(ceremony_id, Some(format!("Commit failed: {e}")))
                .await;
            return Err(());
        }

        if let Err(e) = self
            .signing_service
            .commit_key_rotation(&self.authority_id, new_epoch)
            .await
        {
            tracing::error!(
                ceremony_id = %ceremony_id,
                new_epoch,
                error = %e,
                "Failed to update guardian signing context"
            );
            let _ = self
                .ceremony_runner
                .abort(ceremony_id, Some(format!("Commit failed: {e}")))
                .await;
            return Err(());
        }

        // Create guardian bindings
        let mut bindings = Vec::new();
        for participant in &ceremony_state.participants {
            let aura_core::threshold::ParticipantIdentity::Guardian(guardian_id) = participant
            else {
                continue;
            };

            let binding_hash = aura_core::Hash32(hash::hash(
                format!(
                    "guardian-binding:{}:{}:{}:{}",
                    ceremony_id, self.authority_id, guardian_id, new_epoch
                )
                .as_bytes(),
            ));

            bindings.push(RelationalFact::Protocol(
                ProtocolRelationalFact::GuardianBinding {
                    account_id: self.authority_id,
                    guardian_id: *guardian_id,
                    binding_hash,
                },
            ));
        }

        if !bindings.is_empty() {
            if let Err(e) = self.effects.commit_relational_facts(bindings).await {
                tracing::error!(
                    ceremony_id = %ceremony_id,
                    error = %e,
                    "Failed to commit GuardianBinding facts"
                );
                let _ = self
                    .ceremony_runner
                    .abort(
                        ceremony_id,
                        Some(format!("Failed to commit guardian bindings: {e}")),
                    )
                    .await;
                return Err(());
            }
        }

        let _ = self
            .ceremony_runner
            .commit(ceremony_id, CeremonyCommitMetadata::default())
            .await;
        Ok(())
    }
}
