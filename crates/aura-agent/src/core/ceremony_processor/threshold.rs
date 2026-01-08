//! Device Threshold Key Handler
//!
//! Processes device threshold key ceremony envelopes:
//! - `application/aura-device-threshold-key-package`: Key package distribution
//! - `application/aura-device-threshold-acceptance`: Acceptance acknowledgment

use super::ProcessResult;
use crate::runtime::effects::AuraEffectSystem;
use crate::runtime::services::ceremony_runner::{CeremonyCommitMetadata, CeremonyRunner};
use crate::runtime::services::CeremonyTracker;
use crate::ThresholdSigningService;
use aura_core::effects::transport::TransportEnvelope;
use aura_core::effects::{SecureStorageCapability, SecureStorageEffects, SecureStorageLocation};
use aura_core::identifiers::CeremonyId;
use aura_core::{AuthorityId, DeviceId};

/// Handles device threshold key ceremony messages
pub struct ThresholdHandler<'a> {
    authority_id: AuthorityId,
    effects: &'a AuraEffectSystem,
    ceremony_tracker: &'a CeremonyTracker,
    ceremony_runner: &'a CeremonyRunner,
    #[allow(dead_code)]
    signing_service: &'a ThresholdSigningService,
}

impl<'a> ThresholdHandler<'a> {
    /// Create a new threshold handler
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

    /// Handle a device threshold key package envelope
    pub async fn handle_key_package(&self, envelope: &TransportEnvelope) -> ProcessResult {
        use aura_core::effects::TransportEffects;
        use base64::Engine;

        let authority_id = envelope.destination;

        let (Some(ceremony_id), Some(pending_epoch_str), Some(initiator_device_id_str)) = (
            envelope.metadata.get("ceremony-id"),
            envelope.metadata.get("pending-epoch"),
            envelope.metadata.get("initiator-device-id"),
        ) else {
            tracing::warn!("Malformed device threshold key package envelope");
            return ProcessResult::Skip;
        };

        let pending_epoch: u64 = match pending_epoch_str.parse() {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!(
                    ceremony_id = %ceremony_id,
                    pending_epoch = %pending_epoch_str,
                    error = %e,
                    "Invalid pending epoch in device threshold key package"
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
                    "Invalid initiator device id in device threshold key package"
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
                    "Ignoring device threshold key package for a different device"
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
                "Failed to store device threshold key package"
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
            h.update(b"DEVICE_THRESHOLD_CONTEXT");
            h.update(&authority_id.to_bytes());
            h.update(ceremony_id.as_bytes());
            h.finalize()
        };
        let ceremony_context = aura_core::identifiers::ContextId::new_from_entropy(context_entropy);

        let mut metadata = std::collections::HashMap::new();
        metadata.insert(
            "content-type".to_string(),
            "application/aura-device-threshold-acceptance".to_string(),
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
                "Failed to send device threshold acceptance"
            );
        }

        ProcessResult::Processed
    }

    /// Handle a device threshold acceptance envelope
    pub async fn handle_acceptance(&self, envelope: &TransportEnvelope) -> ProcessResult {
        let (Some(ceremony_id), Some(acceptor_device_id_str)) = (
            envelope.metadata.get("ceremony-id"),
            envelope.metadata.get("acceptor-device-id"),
        ) else {
            tracing::warn!("Malformed device threshold acceptance envelope");
            return ProcessResult::Skip;
        };

        let acceptor_device_id: DeviceId = match acceptor_device_id_str.parse() {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!(
                    ceremony_id = %ceremony_id,
                    acceptor_device_id = %acceptor_device_id_str,
                    error = %e,
                    "Invalid acceptor device id in threshold acceptance"
                );
                return ProcessResult::Skip;
            }
        };
        let ceremony_id = CeremonyId::new(ceremony_id.clone());

        let participant = aura_core::threshold::ParticipantIdentity::device(acceptor_device_id);
        let threshold_reached = match self
            .ceremony_runner
            .record_response(&ceremony_id, participant)
            .await
        {
            Ok(reached) => reached,
            Err(e) => {
                tracing::warn!(
                    ceremony_id = %ceremony_id,
                    acceptor = %acceptor_device_id,
                    error = %e,
                    "Failed to mark device as accepted for threshold"
                );
                return ProcessResult::Skip;
            }
        };

        if threshold_reached {
            // When threshold is reached, send commit to all participants
            if let Err(e) = self.send_commit_to_participants(&ceremony_id).await {
                tracing::error!(
                    ceremony_id = %ceremony_id,
                    error = ?e,
                    "Failed to send threshold commit messages"
                );
            }
            let _ = self
                .ceremony_runner
                .commit(&ceremony_id, CeremonyCommitMetadata::default())
                .await;
        }

        ProcessResult::Processed
    }

    /// Send commit messages to all ceremony participants
    async fn send_commit_to_participants(&self, ceremony_id: &CeremonyId) -> Result<(), String> {
        use aura_core::effects::TransportEffects;

        let ceremony_state = self
            .ceremony_tracker
            .get(ceremony_id)
            .await
            .map_err(|e| format!("Failed to get ceremony state: {e}"))?;

        let context_entropy = {
            let mut h = aura_core::hash::hasher();
            h.update(b"DEVICE_THRESHOLD_CONTEXT");
            h.update(&self.authority_id.to_bytes());
            h.update(ceremony_id.as_str().as_bytes());
            h.finalize()
        };
        let ceremony_context = aura_core::identifiers::ContextId::new_from_entropy(context_entropy);

        for participant in &ceremony_state.participants {
            let aura_core::threshold::ParticipantIdentity::Device(device_id) = participant else {
                continue;
            };

            let mut metadata = std::collections::HashMap::new();
            metadata.insert(
                "content-type".to_string(),
                "application/aura-device-threshold-commit".to_string(),
            );
            metadata.insert("ceremony-id".to_string(), ceremony_id.to_string());
            metadata.insert(
                "new-epoch".to_string(),
                ceremony_state.new_epoch.to_string(),
            );
            metadata.insert(
                "aura-destination-device-id".to_string(),
                device_id.to_string(),
            );

            let commit_envelope = aura_core::effects::TransportEnvelope {
                destination: self.authority_id,
                source: self.authority_id,
                context: ceremony_context,
                payload: Vec::new(),
                metadata,
                receipt: None,
            };

            if let Err(e) = self.effects.send_envelope(commit_envelope).await {
                tracing::warn!(
                    ceremony_id = %ceremony_id,
                    device_id = %device_id,
                    error = %e,
                    "Failed to send threshold commit to device"
                );
            }
        }

        Ok(())
    }
}
