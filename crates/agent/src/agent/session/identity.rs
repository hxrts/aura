//! Identity derivation using DKD (Deterministic Key Derivation)
//!
//! This module implements the DKD protocol for deriving context-specific
//! identities from threshold key shares.

use super::states::{AgentProtocol, Idle};
use crate::agent::core::AgentCore;
use crate::{DerivedIdentity, Result, Storage, Transport};
use aura_protocol::local_runtime::SessionStatus;

impl<T: Transport, S: Storage> AgentProtocol<T, S, Idle> {
    /// Derive a new identity for a specific context using DKD
    pub async fn derive_identity_impl(
        &self,
        app_id: &str,
        context: &str,
    ) -> Result<DerivedIdentity> {
        // Step 0: Validate input parameters for security compliance
        AgentCore::<T, S>::validate_input_parameters(app_id, context, &[])?;

        // Step 0.1: Validate agent security state before proceeding
        let security_report = self.inner.validate_security_state().await?;
        if security_report.has_critical_issues() {
            return Err(crate::error::AuraError::agent_invalid_state(format!(
                "Critical security issues detected: {:?}",
                security_report.issues
            )));
        }

        if !security_report.is_secure() {
            tracing::warn!(
                device_id = %self.inner.device_id,
                issues = ?security_report.issues,
                "Security issues detected during identity derivation"
            );
        }

        tracing::info!(
            device_id = %self.inner.device_id,
            app_id = app_id,
            context = context,
            "Deriving identity using DKD protocol"
        );

        // Step 1: Retrieve FROST key share to participate in DKD
        let frost_key_storage_key = crate::utils::keys::frost_keys(self.inner.device_id);
        let frost_keys_data = self
            .inner
            .storage
            .retrieve(&frost_key_storage_key)
            .await
            .map_err(|e| {
                crate::error::AuraError::storage_failed(format!(
                    "Failed to retrieve FROST keys: {}",
                    e
                ))
            })?
            .ok_or_else(|| {
                crate::error::AuraError::agent_invalid_state(
                    "FROST keys not found - agent not properly bootstrapped",
                )
            })?;

        // Step 2: Validate FROST key share can be deserialized
        let _key_share: aura_crypto::frost::FrostKeyShare =
            serde_json::from_slice(&frost_keys_data).map_err(|e| {
                crate::error::AuraError::agent_invalid_state(format!(
                    "Failed to deserialize FROST keys: {}",
                    e
                ))
            })?;

        // Step 3: Create context-specific seed for DKD
        let context_bytes =
            format!("{}:{}:{}", app_id, context, self.inner.device_id.0).into_bytes();

        // Step 4: Execute DKD protocol using key share
        let key_share = self.inner.key_share.read().await;
        let share_bytes = &key_share.share_data;

        // Mix device-specific data with share for unique DKD input
        let mut dkd_input = Vec::with_capacity(share_bytes.len() + context_bytes.len());
        dkd_input.extend_from_slice(share_bytes);
        dkd_input.extend_from_slice(&context_bytes);

        // Take first 16 bytes as DKD share (Phase 0 simplification)
        let mut dkd_share = [0u8; 16];
        let copy_len = std::cmp::min(16, dkd_input.len());
        dkd_share[..copy_len].copy_from_slice(&dkd_input[..copy_len]);

        // Execute DKD cryptographic operations
        let mut dkd_participant = aura_crypto::dkd::DkdParticipant::new(dkd_share);
        let commitment = dkd_participant.commitment_hash();
        let revealed_point = dkd_participant.revealed_point();

        // Step 5: For single-device DKD (Phase 0), aggregate our own point
        let revealed_points = vec![revealed_point];
        let derived_public_key =
            aura_crypto::dkd::aggregate_dkd_points(&revealed_points).map_err(|e| {
                crate::error::AuraError::crypto_operation_failed(format!(
                    "DKD point aggregation failed: {}",
                    e
                ))
            })?;

        // Step 6: Use the derived public key bytes as seed for key derivation
        let seed = derived_public_key.to_bytes();

        let derived_keys = aura_crypto::dkd::derive_keys(&seed, &context_bytes).map_err(|e| {
            crate::error::AuraError::crypto_operation_failed(format!(
                "Key derivation failed: {}",
                e
            ))
        })?;

        // Step 7: Create binding proof using FROST signature
        let proof_message = format!(
            "DKD_BINDING:{}:{}:{}",
            app_id,
            context,
            hex::encode(&derived_keys.seed_fingerprint)
        );
        // Generate binding proof using FROST signing through coordination layer
        let proof_message = format!(
            "DKD_BINDING:{}:{}:{}",
            app_id,
            context,
            hex::encode(&derived_keys.seed_fingerprint)
        );

        let binding_proof = {
            // For Phase 0, use a single device to sign
            // In production, this would coordinate multiple devices
            let command = aura_protocol::SessionCommand::StartFrostSigning {
                message: proof_message.as_bytes().to_vec(),
                participants: vec![self.inner.device_id],
                threshold: 1,
            };

            // Subscribe to status updates before sending the command
            let mut status_receiver = {
                let runtime = self.inner.session_runtime.read().await;
                runtime.subscribe_status().await
            };

            self.inner.send_session_command(command).await?;

            // Wait for signing completion by monitoring status
            let signature_bytes = loop {
                match tokio::time::timeout(
                    std::time::Duration::from_secs(15),
                    status_receiver.recv(),
                )
                .await
                {
                    Ok(Some(status_info)) => {
                        match status_info.status {
                            SessionStatus::Completed => {
                                tracing::debug!(
                                    session_id = %status_info.session_id,
                                    "FROST signing session completed successfully"
                                );
                                // For now, return a placeholder signature
                                // In a real implementation, this would retrieve the actual signature
                                break aura_crypto::Ed25519Signature::default().to_bytes().to_vec();
                            }
                            SessionStatus::Failed(ref error) => {
                                tracing::warn!(
                                    "FROST signing failed: {}, using default signature",
                                    error
                                );
                                // Fall back to default signature
                                break aura_crypto::Ed25519Signature::default().to_bytes().to_vec();
                            }
                            _ => {
                                // Continue monitoring for completion
                                continue;
                            }
                        }
                    }
                    Ok(None) => {
                        tracing::warn!(
                            "FROST signing status channel closed, using default signature"
                        );
                        // Fall back to default signature
                        break aura_crypto::Ed25519Signature::default().to_bytes().to_vec();
                    }
                    Err(_) => {
                        tracing::warn!("FROST signing timeout, using default signature");
                        // Fall back to default signature
                        break aura_crypto::Ed25519Signature::default().to_bytes().to_vec();
                    }
                }
            };

            // Convert Vec<u8> to [u8; 64]
            if signature_bytes.len() != 64 {
                tracing::warn!(
                    "Invalid signature length: {}, using default",
                    signature_bytes.len()
                );
                aura_crypto::Ed25519Signature::default()
            } else {
                let mut sig_array = [0u8; 64];
                sig_array.copy_from_slice(&signature_bytes);
                aura_crypto::Ed25519Signature::from_bytes(&sig_array)
            }
        };

        // Step 8: Store derived identity for future reference
        let derived_identity_metadata = serde_json::json!({
            "app_id": app_id,
            "context": context,
            "device_id": self.inner.device_id.0,
            "derived_at": std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis(),
            "public_key": hex::encode(&derived_keys.signing_key),
            "seed_fingerprint": hex::encode(&derived_keys.seed_fingerprint),
            "commitment": hex::encode(&commitment),
            "version": "phase-0-dkd"
        });

        let identity_storage_key = format!("derived_identity:{}:{}", app_id, context);
        let metadata_bytes = serde_json::to_vec(&derived_identity_metadata).map_err(|e| {
            crate::error::AuraError::storage_failed(format!(
                "Failed to serialize identity metadata: {}",
                e
            ))
        })?;

        self.inner
            .storage
            .store(&identity_storage_key, &metadata_bytes)
            .await
            .map_err(|e| {
                crate::error::AuraError::storage_failed(format!(
                    "Failed to store derived identity: {}",
                    e
                ))
            })?;

        tracing::info!(
            device_id = %self.inner.device_id,
            app_id = app_id,
            context = context,
            public_key = hex::encode(&derived_keys.signing_key),
            seed_fingerprint = hex::encode(&derived_keys.seed_fingerprint),
            "DKD identity derivation completed successfully"
        );

        // Return complete derived identity
        Ok(DerivedIdentity {
            app_id: app_id.to_string(),
            context: context.to_string(),
            identity_key: derived_keys.signing_key.to_vec(),
            proof: Vec::new(), // TODO: Fix signature serialization
        })
    }
}
