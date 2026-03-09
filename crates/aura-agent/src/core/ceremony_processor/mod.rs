//! Ceremony Processing Module
//!
//! Extracts ceremony processing logic from the main API into focused handlers.
//! Each handler module is responsible for a specific ceremony type:
//! - `guardian`: Guardian acceptance processing
//! - `enrollment`: Device enrollment key package and acceptance
//! - `threshold`: Device threshold key package and acceptance
//! - `commit`: Final ceremony commit processing
//!
//! ## Architecture
//!
//! The `CeremonyProcessor` coordinates between:
//! - `CeremonyTracker`: Tracks ceremony state and threshold progress
//! - `ThresholdSigningService`: Manages threshold key operations
//! - Effect system: Transport, tree operations, key management

mod commit;
mod enrollment;
mod guardian;
mod threshold;

use crate::runtime::effects::AuraEffectSystem;
use crate::runtime::services::ceremony_runner::CeremonyRunner;
use crate::runtime::services::{CeremonyTracker, ReconfigurationManager};
use crate::AgentResult;
use crate::ThresholdSigningService;
use aura_core::effects::transport::TransportEnvelope;
use aura_core::AuthorityId;

pub use commit::CommitHandler;
pub use enrollment::EnrollmentHandler;
pub use guardian::GuardianHandler;
pub use threshold::ThresholdHandler;

/// Result of processing a single ceremony envelope
#[derive(Debug)]
pub enum ProcessResult {
    /// Envelope was processed successfully
    Processed,
    /// Ceremony reached threshold and was committed
    Committed,
    /// Envelope was not for ceremony processing (requeue it)
    NotCeremony,
    /// Envelope was malformed or processing failed (skip it)
    Skip,
}

/// Coordinates ceremony processing across all handler types
pub struct CeremonyProcessor<'a> {
    authority_id: AuthorityId,
    effects: &'a AuraEffectSystem,
    ceremony_tracker: CeremonyTracker,
    ceremony_runner: CeremonyRunner,
    signing_service: ThresholdSigningService,
    reconfiguration_manager: ReconfigurationManager,
}

impl<'a> CeremonyProcessor<'a> {
    /// Create a new ceremony processor
    pub fn new(
        authority_id: AuthorityId,
        effects: &'a AuraEffectSystem,
        ceremony_tracker: CeremonyTracker,
        ceremony_runner: CeremonyRunner,
        signing_service: ThresholdSigningService,
        reconfiguration_manager: ReconfigurationManager,
    ) -> Self {
        Self {
            authority_id,
            effects,
            ceremony_tracker,
            ceremony_runner,
            signing_service,
            reconfiguration_manager,
        }
    }

    /// Process incoming ceremony envelopes
    ///
    /// Returns the number of acceptances processed and ceremonies completed.
    pub async fn process_all(&self) -> AgentResult<(usize, usize)> {
        use aura_core::effects::TransportEffects;

        let mut acceptance_count = 0usize;
        let mut completed_count = 0usize;

        loop {
            let envelope = match self.effects.receive_envelope().await {
                Ok(env) => env,
                Err(aura_core::effects::TransportError::NoMessage) => break,
                Err(e) => {
                    tracing::warn!("Error receiving ceremony envelope: {}", e);
                    break;
                }
            };

            tracing::info!(
                authority_id = %self.authority_id,
                source = %envelope.source,
                destination = %envelope.destination,
                content_type = envelope.metadata.get("content-type").cloned().unwrap_or_default(),
                ceremony_id = envelope.metadata.get("ceremony-id").cloned().unwrap_or_default(),
                acceptor_device_id = envelope.metadata.get("acceptor-device-id").cloned().unwrap_or_default(),
                destination_device_id = envelope.metadata.get("aura-destination-device-id").cloned().unwrap_or_default(),
                "CeremonyProcessor received envelope"
            );
            eprintln!(
                "[ceremony-envelope] authority_id={};source={};destination={};content_type={};ceremony_id={};acceptor_device_id={};destination_device_id={}",
                self.authority_id,
                envelope.source,
                envelope.destination,
                envelope
                    .metadata
                    .get("content-type")
                    .cloned()
                    .unwrap_or_default(),
                envelope.metadata.get("ceremony-id").cloned().unwrap_or_default(),
                envelope
                    .metadata
                    .get("acceptor-device-id")
                    .cloned()
                    .unwrap_or_default(),
                envelope
                    .metadata
                    .get("aura-destination-device-id")
                    .cloned()
                    .unwrap_or_default(),
            );

            match self.process_envelope(envelope).await {
                ProcessResult::Processed => acceptance_count += 1,
                ProcessResult::Committed => {
                    acceptance_count += 1;
                    completed_count += 1;
                }
                ProcessResult::NotCeremony => break,
                ProcessResult::Skip => continue,
            }
        }

        Ok((acceptance_count, completed_count))
    }

    /// Process a single ceremony envelope
    async fn process_envelope(&self, envelope: TransportEnvelope) -> ProcessResult {
        let Some(content_type) = envelope.metadata.get("content-type").cloned() else {
            self.effects.requeue_envelope(envelope);
            return ProcessResult::NotCeremony;
        };

        match content_type.as_str() {
            "application/aura-guardian-acceptance" => {
                GuardianHandler::new(
                    self.authority_id,
                    self.effects,
                    &self.ceremony_tracker,
                    &self.ceremony_runner,
                    &self.signing_service,
                    &self.reconfiguration_manager,
                )
                .handle(&envelope)
                .await
            }
            "application/aura-device-enrollment-key-package" => {
                EnrollmentHandler::new(
                    self.authority_id,
                    self.effects,
                    &self.ceremony_tracker,
                    &self.ceremony_runner,
                    &self.signing_service,
                    &self.reconfiguration_manager,
                )
                .handle_key_package(&envelope)
                .await
            }
            "application/aura-device-enrollment-acceptance" => {
                EnrollmentHandler::new(
                    self.authority_id,
                    self.effects,
                    &self.ceremony_tracker,
                    &self.ceremony_runner,
                    &self.signing_service,
                    &self.reconfiguration_manager,
                )
                .handle_acceptance(&envelope)
                .await
            }
            "application/aura-device-threshold-key-package" => {
                ThresholdHandler::new(
                    self.authority_id,
                    self.effects,
                    &self.ceremony_tracker,
                    &self.ceremony_runner,
                    &self.signing_service,
                    &self.reconfiguration_manager,
                )
                .handle_key_package(&envelope)
                .await
            }
            "application/aura-device-threshold-acceptance" => {
                ThresholdHandler::new(
                    self.authority_id,
                    self.effects,
                    &self.ceremony_tracker,
                    &self.ceremony_runner,
                    &self.signing_service,
                    &self.reconfiguration_manager,
                )
                .handle_acceptance(&envelope)
                .await
            }
            "application/aura-device-enrollment-commit"
            | "application/aura-device-threshold-commit" => {
                CommitHandler::new(
                    self.authority_id,
                    self.effects,
                    &self.ceremony_tracker,
                    &self.ceremony_runner,
                    &self.signing_service,
                    &self.reconfiguration_manager,
                )
                .handle(&envelope, &content_type)
                .await
            }
            _ => {
                self.effects.requeue_envelope(envelope);
                ProcessResult::NotCeremony
            }
        }
    }
}
