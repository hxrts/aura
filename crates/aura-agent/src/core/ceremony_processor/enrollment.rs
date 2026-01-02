//! Device Enrollment Handler
//!
//! Processes device enrollment ceremony envelopes:
//! - `application/aura-device-enrollment-key-package`: Key package distribution
//! - `application/aura-device-enrollment-acceptance`: Acceptance acknowledgment

use super::ProcessResult;
use crate::runtime::effects::AuraEffectSystem;
use crate::runtime::services::CeremonyTracker;
use crate::ThresholdSigningService;
use aura_core::effects::transport::TransportEnvelope;
use aura_core::effects::{SecureStorageCapability, SecureStorageEffects, SecureStorageLocation};
use aura_core::{AuthorityId, DeviceId};

/// Handles device enrollment ceremony messages
pub struct EnrollmentHandler<'a> {
    #[allow(dead_code)]
    authority_id: AuthorityId,
    effects: &'a AuraEffectSystem,
    ceremony_tracker: &'a CeremonyTracker,
    #[allow(dead_code)]
    signing_service: &'a ThresholdSigningService,
}

impl<'a> EnrollmentHandler<'a> {
    /// Create a new enrollment handler
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

    /// Handle a device enrollment key package envelope
    pub async fn handle_key_package(&self, envelope: &TransportEnvelope) -> ProcessResult {
        use aura_core::effects::TransportEffects;
        use base64::Engine;

        let authority_id = envelope.destination;

        let (Some(ceremony_id), Some(pending_epoch_str), Some(initiator_device_id_str)) = (
            envelope.metadata.get("ceremony-id"),
            envelope.metadata.get("pending-epoch"),
            envelope.metadata.get("initiator-device-id"),
        ) else {
            tracing::warn!("Malformed device enrollment key package envelope");
            return ProcessResult::Skip;
        };

        let pending_epoch: u64 = match pending_epoch_str.parse() {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!(
                    ceremony_id = %ceremony_id,
                    pending_epoch = %pending_epoch_str,
                    error = %e,
                    "Invalid pending epoch in device enrollment key package"
                );
                return ProcessResult::Skip;
            }
        };

        let initiator_device_id: DeviceId = match initiator_device_id_str.parse() {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!(
                    ceremony_id = %ceremony_id,
                    initiator_device_id = %initiator_device_id_str,
                    error = %e,
                    "Invalid initiator device id in device enrollment key package"
                );
                return ProcessResult::Skip;
            }
        };

        // Get self device ID from config
        let self_device_id = self.effects.device_id();
        if let Some(participant_device_id) = envelope.metadata.get("participant-device-id") {
            if participant_device_id != &self_device_id.to_string() {
                tracing::warn!(
                    ceremony_id = %ceremony_id,
                    expected_device_id = %self_device_id,
                    got_device_id = %participant_device_id,
                    "Ignoring device enrollment key package for a different device"
                );
                return ProcessResult::Skip;
            }
        }

        // Store the key package
        let participant = aura_core::threshold::ParticipantIdentity::device(self_device_id);
        let location = SecureStorageLocation::with_sub_key(
            "participant_shares",
            format!("{}/{}", authority_id, pending_epoch),
            participant.storage_key(),
        );

        if let Err(e) = self
            .effects
            .secure_store(
                &location,
                &envelope.payload,
                &[
                    SecureStorageCapability::Read,
                    SecureStorageCapability::Write,
                ],
            )
            .await
        {
            tracing::warn!(
                ceremony_id = %ceremony_id,
                error = %e,
                "Failed to store device enrollment key package"
            );
            return ProcessResult::Skip;
        }

        // Store threshold config and pubkey if provided
        if let (Some(config_b64), Some(pubkey_b64)) = (
            envelope.metadata.get("threshold-config"),
            envelope.metadata.get("threshold-pubkey"),
        ) {
            if let (Ok(config_bytes), Ok(pubkey_bytes)) = (
                base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(config_b64),
                base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(pubkey_b64),
            ) {
                let config_location = SecureStorageLocation::with_sub_key(
                    "threshold_config",
                    format!("{}", authority_id),
                    format!("{}", pending_epoch),
                );
                let pubkey_location = SecureStorageLocation::with_sub_key(
                    "threshold_pubkey",
                    format!("{}", authority_id),
                    format!("{}", pending_epoch),
                );

                if let Err(e) = self
                    .effects
                    .secure_store(
                        &config_location,
                        &config_bytes,
                        &[
                            SecureStorageCapability::Read,
                            SecureStorageCapability::Write,
                        ],
                    )
                    .await
                {
                    tracing::warn!(
                        ceremony_id = %ceremony_id,
                        error = %e,
                        "Failed to store threshold config metadata"
                    );
                }

                if let Err(e) = self
                    .effects
                    .secure_store(
                        &pubkey_location,
                        &pubkey_bytes,
                        &[
                            SecureStorageCapability::Read,
                            SecureStorageCapability::Write,
                        ],
                    )
                    .await
                {
                    tracing::warn!(
                        ceremony_id = %ceremony_id,
                        error = %e,
                        "Failed to store threshold public key package"
                    );
                }
            }
        }

        // Send acceptance acknowledgment
        let context_entropy = {
            let mut h = aura_core::hash::hasher();
            h.update(b"DEVICE_ENROLLMENT_CONTEXT");
            h.update(&authority_id.to_bytes());
            h.update(ceremony_id.as_bytes());
            h.finalize()
        };
        let ceremony_context = aura_core::identifiers::ContextId::new_from_entropy(context_entropy);

        let mut metadata = std::collections::HashMap::new();
        metadata.insert(
            "content-type".to_string(),
            "application/aura-device-enrollment-acceptance".to_string(),
        );
        metadata.insert("ceremony-id".to_string(), ceremony_id.clone());
        metadata.insert("acceptor-device-id".to_string(), self_device_id.to_string());
        metadata.insert(
            "aura-destination-device-id".to_string(),
            initiator_device_id.to_string(),
        );

        let response = aura_core::effects::TransportEnvelope {
            destination: authority_id,
            source: authority_id,
            context: ceremony_context,
            payload: Vec::new(),
            metadata,
            receipt: None,
        };

        if let Err(e) = self.effects.send_envelope(response).await {
            tracing::warn!(
                ceremony_id = %ceremony_id,
                error = %e,
                "Failed to send device enrollment acceptance"
            );
        }

        ProcessResult::Processed
    }

    /// Handle a device enrollment acceptance envelope
    pub async fn handle_acceptance(&self, envelope: &TransportEnvelope) -> ProcessResult {
        let (Some(ceremony_id), Some(acceptor_device_id_str)) = (
            envelope.metadata.get("ceremony-id"),
            envelope.metadata.get("acceptor-device-id"),
        ) else {
            tracing::warn!("Malformed device enrollment acceptance envelope");
            return ProcessResult::Skip;
        };

        let acceptor_device_id: DeviceId = match acceptor_device_id_str.parse() {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!(
                    ceremony_id = %ceremony_id,
                    acceptor_device_id = %acceptor_device_id_str,
                    error = %e,
                    "Invalid acceptor device id in enrollment acceptance"
                );
                return ProcessResult::Skip;
            }
        };

        let participant = aura_core::threshold::ParticipantIdentity::device(acceptor_device_id);
        let threshold_reached = match self
            .ceremony_tracker
            .mark_accepted(ceremony_id, participant)
            .await
        {
            Ok(reached) => reached,
            Err(e) => {
                tracing::warn!(
                    ceremony_id = %ceremony_id,
                    acceptor = %acceptor_device_id,
                    error = %e,
                    "Failed to mark device as accepted for enrollment"
                );
                return ProcessResult::Skip;
            }
        };

        if threshold_reached {
            // Enrollment ceremonies complete when all devices have their key packages
            // The commit happens via a separate commit message
            tracing::info!(
                ceremony_id = %ceremony_id,
                "Device enrollment threshold reached, awaiting commit"
            );
        }

        ProcessResult::Processed
    }
}
