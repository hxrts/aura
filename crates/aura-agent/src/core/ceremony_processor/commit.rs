//! Commit Handler
//!
//! Processes device commit envelopes:
//! - `application/aura-device-enrollment-commit`: Enrollment ceremony commit
//! - `application/aura-device-threshold-commit`: Threshold ceremony commit

use super::ProcessResult;
use crate::ThresholdSigningService;
use crate::runtime::effects::AuraEffectSystem;
use crate::runtime::services::CeremonyTracker;
use aura_core::effects::transport::TransportEnvelope;
use aura_core::effects::ThresholdSigningEffects;
use aura_core::AuthorityId;

/// Handles device commit messages
pub struct CommitHandler<'a> {
    #[allow(dead_code)]
    authority_id: AuthorityId,
    effects: &'a AuraEffectSystem,
    #[allow(dead_code)]
    ceremony_tracker: &'a CeremonyTracker,
    signing_service: &'a ThresholdSigningService,
}

impl<'a> CommitHandler<'a> {
    /// Create a new commit handler
    pub fn new(
        authority_id: AuthorityId,
        effects: &'a AuraEffectSystem,
        ceremony_tracker: &'a CeremonyTracker,
        signing_service: &'a ThresholdSigningService,
    ) -> Self {
        Self {
            authority_id,
            effects,
            ceremony_tracker,
            signing_service,
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

        ProcessResult::Committed
    }
}
