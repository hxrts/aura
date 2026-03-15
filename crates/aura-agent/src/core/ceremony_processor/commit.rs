//! Commit Handler
//!
//! Processes device commit envelopes:
//! - `application/aura-device-enrollment-commit`: Enrollment ceremony commit
//! - `application/aura-device-threshold-commit`: Threshold ceremony commit

use super::ProcessResult;
use crate::runtime::effects::AuraEffectSystem;
use crate::runtime::services::ceremony_runner::{CeremonyCommitMetadata, CeremonyRunner};
use crate::runtime::services::{CeremonyTracker, ReconfigurationManager};
use crate::ThresholdSigningService;
use aura_core::effects::transport::TransportEnvelope;
use aura_core::effects::ThresholdSigningEffects;
use aura_core::util::serialization::from_slice;
use aura_core::{AttestedOp, AuthorityId};
use aura_protocol::effects::TreeEffects;

/// Handles device commit messages
pub struct CommitHandler<'a> {
    #[allow(dead_code)]
    authority_id: AuthorityId,
    effects: &'a AuraEffectSystem,
    #[allow(dead_code)]
    ceremony_tracker: &'a CeremonyTracker,
    ceremony_runner: &'a CeremonyRunner,
    signing_service: &'a ThresholdSigningService,
    #[allow(dead_code)]
    reconfiguration: &'a ReconfigurationManager,
}

impl<'a> CommitHandler<'a> {
    /// Create a new commit handler
    pub fn new(
        authority_id: AuthorityId,
        effects: &'a AuraEffectSystem,
        ceremony_tracker: &'a CeremonyTracker,
        ceremony_runner: &'a CeremonyRunner,
        signing_service: &'a ThresholdSigningService,
        reconfiguration: &'a ReconfigurationManager,
    ) -> Self {
        Self {
            authority_id,
            effects,
            ceremony_tracker,
            ceremony_runner,
            signing_service,
            reconfiguration,
        }
    }

    /// Handle a device commit envelope (enrollment or threshold)
    pub async fn handle(&self, envelope: &TransportEnvelope, content_type: &str) -> ProcessResult {
        let Some(new_epoch_str) = envelope.metadata.get("new-epoch") else {
            tracing::warn!(
                content_type = %content_type,
                "Missing new-epoch in device commit envelope"
            );
            return ProcessResult::Skip;
        };
        let ceremony_id = envelope
            .metadata
            .get("ceremony-id")
            .map(|id| aura_core::types::identifiers::CeremonyId::new(id.clone()));

        let new_epoch: u64 = match new_epoch_str.parse() {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!(
                    new_epoch = %new_epoch_str,
                    error = %e,
                    "Invalid epoch in device commit notice"
                );
                return ProcessResult::Skip;
            }
        };

        let authority_id = envelope.destination;

        if content_type == "application/aura-device-enrollment-commit"
            && !envelope.payload.is_empty()
        {
            let attested: AttestedOp = match from_slice(&envelope.payload) {
                Ok(attested) => attested,
                Err(e) => {
                    tracing::warn!(
                        content_type = %content_type,
                        error = %e,
                        "Failed to decode enrollment commit op"
                    );
                    return ProcessResult::Skip;
                }
            };

            if let Err(e) = self.effects.apply_attested_op(attested).await {
                tracing::warn!(
                    content_type = %content_type,
                    error = %e,
                    "Failed to apply enrollment commit op"
                );
                return ProcessResult::Skip;
            }

            tracing::info!(
                content_type = %content_type,
                authority_id = %authority_id,
                "Applied enrollment commit op"
            );
            tracing::debug!(
                authority_id = %authority_id,
                content_type = %content_type,
                "device-enrollment-commit-applied"
            );
        } else if content_type == "application/aura-device-enrollment-commit" {
            tracing::info!(
                content_type = %content_type,
                authority_id = %authority_id,
                "Received enrollment commit without leaf op payload"
            );
            tracing::debug!(
                authority_id = %authority_id,
                content_type = %content_type,
                "device-enrollment-commit-empty"
            );
        }

        // Commit key rotation via effects
        if let Err(e) = self
            .effects
            .commit_key_rotation(&authority_id, new_epoch)
            .await
        {
            tracing::warn!(
                authority_id = %authority_id,
                new_epoch,
                error = %e,
                "Failed to activate committed device threshold epoch"
            );
            return ProcessResult::Skip;
        }

        // Update signing service
        if let Err(e) = self
            .signing_service
            .commit_key_rotation(&authority_id, new_epoch)
            .await
        {
            tracing::warn!(
                authority_id = %authority_id,
                new_epoch,
                error = %e,
                "Failed to update signing context for committed device threshold epoch"
            );
        }

        if let Some(ceremony_id) = ceremony_id.as_ref() {
            let _ = self
                .ceremony_runner
                .commit(ceremony_id, CeremonyCommitMetadata::default())
                .await;
        }

        ProcessResult::Committed
    }
}
