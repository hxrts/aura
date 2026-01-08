//! Device Enrollment Handler
//!
//! Processes device enrollment ceremony envelopes:
//! - `application/aura-device-enrollment-key-package`: Key package distribution
//! - `application/aura-device-enrollment-acceptance`: Acceptance acknowledgment

use super::ProcessResult;
use crate::runtime::effects::AuraEffectSystem;
use crate::runtime::services::ceremony_runner::{CeremonyCommitMetadata, CeremonyRunner};
use crate::runtime::services::CeremonyTracker;
use crate::ThresholdSigningService;
use aura_core::effects::transport::TransportEnvelope;
use aura_core::effects::{
    SecureStorageCapability, SecureStorageEffects, SecureStorageLocation, ThresholdSigningEffects,
};
use aura_core::identifiers::CeremonyId;
use aura_core::tree::metadata::DeviceLeafMetadata;
use aura_core::tree::LeafRole;
use aura_core::{AttestedOp, AuthorityId, DeviceId, LeafId, LeafNode, NodeIndex, TreeOp};
use aura_protocol::effects::TreeEffects;

/// Handles device enrollment ceremony messages
pub struct EnrollmentHandler<'a> {
    authority_id: AuthorityId,
    effects: &'a AuraEffectSystem,
    ceremony_tracker: &'a CeremonyTracker,
    ceremony_runner: &'a CeremonyRunner,
    #[allow(dead_code)]
    signing_service: &'a ThresholdSigningService,
}

impl<'a> EnrollmentHandler<'a> {
    /// Create a new enrollment handler
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
                    "Failed to mark device as accepted for enrollment"
                );
                return ProcessResult::Skip;
            }
        };

        if threshold_reached {
            if let Err(e) = self.finalize_enrollment(&ceremony_id).await {
                tracing::warn!(
                    ceremony_id = %ceremony_id,
                    error = %e,
                    "Failed to finalize device enrollment locally"
                );
            }
            // When threshold is reached, send commit to all participants
            if let Err(e) = self.send_commit_to_participants(&ceremony_id).await {
                tracing::error!(
                    ceremony_id = %ceremony_id,
                    error = ?e,
                    "Failed to send enrollment commit messages"
                );
            }
        }

        ProcessResult::Processed
    }

    async fn finalize_enrollment(&self, ceremony_id: &CeremonyId) -> Result<(), String> {
        let ceremony_state = self
            .ceremony_tracker
            .get(ceremony_id)
            .await
            .map_err(|e| format!("Failed to load ceremony state: {e}"))?;

        let Some(device_id) = ceremony_state.enrollment_device_id else {
            return Ok(());
        };

        let tree_state = self
            .effects
            .get_current_state()
            .await
            .map_err(|e| format!("Failed to read tree state: {e}"))?;

        if tree_state
            .leaves
            .values()
            .any(|leaf| leaf.device_id == device_id)
        {
            // Leaf already present; still commit the pending epoch below.
            tracing::debug!(
                ceremony_id = %ceremony_id,
                device_id = %device_id,
                "Enrollment leaf already present; skipping add-leaf op"
            );
        } else {
            use aura_core::crypto::tree_signing::{
                public_key_package_from_bytes, share_from_key_package_bytes,
            };
            use aura_core::effects::{
                SecureStorageCapability, SecureStorageEffects, SecureStorageLocation,
                ThresholdSigningEffects,
            };
            use aura_core::threshold::{ParticipantIdentity, SigningContext};

            let participant = ParticipantIdentity::device(device_id);
            let key_location = SecureStorageLocation::with_sub_key(
                "participant_shares",
                format!("{}/{}", self.authority_id, ceremony_state.new_epoch),
                participant.storage_key(),
            );

            let key_package = self
                .effects
                .secure_retrieve(&key_location, &[SecureStorageCapability::Read])
                .await
                .map_err(|e| format!("Failed to load enrollment key package: {e}"))?;

            let share = share_from_key_package_bytes(&key_package)
                .map_err(|e| format!("Failed to decode enrollment key package: {e}"))?;

            let pubkey_location = SecureStorageLocation::with_sub_key(
                "threshold_pubkey",
                format!("{}", self.authority_id),
                format!("{}", ceremony_state.new_epoch),
            );
            let pubkey_bytes = self
                .effects
                .secure_retrieve(&pubkey_location, &[SecureStorageCapability::Read])
                .await
                .map_err(|e| format!("Failed to load enrollment public key package: {e}"))?;

            let public_key_package = public_key_package_from_bytes(&pubkey_bytes)
                .map_err(|e| format!("Failed to decode enrollment public key package: {e}"))?;

            let public_key_bytes = public_key_package
                .signer_public_keys
                .get(&share.identifier)
                .ok_or_else(|| {
                    format!(
                        "Missing verifying share for signer {} in public key package",
                        share.identifier
                    )
                })?
                .clone();

            let next_leaf_id = tree_state
                .leaves
                .keys()
                .map(|leaf_id| leaf_id.0)
                .max()
                .map(|id| id + 1)
                .unwrap_or(0);

            // Build DeviceLeafMetadata with nickname_suggestion from ceremony state
            let device_metadata =
                if let Some(ref suggestion) = ceremony_state.enrollment_nickname_suggestion {
                    DeviceLeafMetadata::with_nickname_suggestion(suggestion)
                } else {
                    DeviceLeafMetadata::new()
                };

            let leaf_metadata = device_metadata
                .encode()
                .map_err(|e| format!("Failed to encode device metadata: {e}"))?;

            let leaf = LeafNode::new(
                LeafId(next_leaf_id),
                device_id,
                LeafRole::Device,
                public_key_bytes,
                leaf_metadata,
            )
            .map_err(|e| format!("Failed to build device leaf: {e}"))?;

            let op_kind = self
                .effects
                .add_leaf(leaf, NodeIndex(0))
                .await
                .map_err(|e| format!("Failed to build add-leaf op: {e}"))?;

            let op = TreeOp {
                parent_epoch: tree_state.epoch,
                parent_commitment: tree_state.root_commitment,
                op: op_kind,
                version: 1,
            };

            let context = SigningContext::self_tree_op(self.authority_id, op.clone());
            let signature = self
                .signing_service
                .sign(context)
                .await
                .map_err(|e| format!("Failed to sign enrollment tree op: {e}"))?;

            let attested = AttestedOp {
                op,
                agg_sig: signature.signature,
                signer_count: signature.signer_count,
            };

            self.effects
                .apply_attested_op(attested)
                .await
                .map_err(|e| format!("Failed to apply device leaf op: {e}"))?;
        }

        // Commit key rotation locally for the initiator device.
        if let Err(e) = self
            .effects
            .commit_key_rotation(&self.authority_id, ceremony_state.new_epoch)
            .await
        {
            tracing::warn!(
                ceremony_id = %ceremony_id,
                error = %e,
                "Failed to commit key rotation on initiator"
            );
        }
        if let Err(e) = self
            .signing_service
            .commit_key_rotation(&self.authority_id, ceremony_state.new_epoch)
            .await
        {
            tracing::warn!(
                ceremony_id = %ceremony_id,
                error = %e,
                "Failed to commit signing context on initiator"
            );
        }

        Ok(())
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
            h.update(b"DEVICE_ENROLLMENT_CONTEXT");
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
                "application/aura-device-enrollment-commit".to_string(),
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
                    "Failed to send enrollment commit to device"
                );
            }
        }

        // Mark ceremony as committed
        if let Err(e) = self
            .ceremony_runner
            .commit(ceremony_id, CeremonyCommitMetadata::default())
            .await
        {
            tracing::warn!(
                ceremony_id = %ceremony_id,
                error = %e,
                "Failed to mark ceremony as committed"
            );
        }

        Ok(())
    }
}
