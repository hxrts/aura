//! Agent bootstrapping and initialization
//!
//! This module provides the bootstrap implementation for transitioning
//! an uninitialized agent to an idle, operational agent.

use super::states::{AgentProtocol, BootstrapConfig, Idle, Uninitialized};
use crate::agent::core::AgentCore;
use crate::utils::ResultExt;
use crate::{Result, Storage, Transport};
use aura_protocol::local_runtime::SessionStatus;
use aura_types::DeviceId;

impl<T: Transport, S: Storage> AgentProtocol<T, S, Uninitialized> {
    /// Create a new uninitialized agent
    pub fn new_uninitialized(core: AgentCore<T, S>) -> Self {
        Self::new(core)
    }

    /// Bootstrap the agent with initial configuration
    ///
    /// This consumes the uninitialized agent and returns an idle agent
    pub async fn bootstrap(self, config: BootstrapConfig) -> Result<AgentProtocol<T, S, Idle>> {
        tracing::info!(
            device_id = %self.inner.device_id,
            account_id = %self.inner.account_id,
            "Bootstrapping agent with config: {:?}", config
        );

        // Step 1: Initialize FROST key shares using threshold configuration via coordination layer
        // Create participant list - for bootstrap, this device is the first participant
        let mut participants = vec![self.inner.device_id];

        // Add additional participants from config if provided
        if let Some(additional_devices) = config.parameters.get("additional_devices") {
            if let Ok(device_list) =
                serde_json::from_value::<Vec<String>>(additional_devices.clone())
            {
                for device_str in device_list {
                    if let Ok(uuid) = uuid::Uuid::parse_str(&device_str) {
                        participants.push(DeviceId(uuid));
                    }
                }
            }
        }

        // Ensure we have enough participants for the threshold
        if participants.len() < config.threshold as usize {
            return Err(crate::error::AuraError::agent_invalid_state(format!(
                "Not enough participants ({}) for threshold ({})",
                participants.len(),
                config.threshold
            )));
        }

        // Send FROST DKG command through session runtime
        let command = aura_protocol::SessionCommand::StartFrostDkg {
            participants: participants.clone(),
            threshold: config.threshold,
        };

        // Subscribe to status updates before sending the command
        let mut status_receiver = {
            let runtime = self.inner.session_runtime.read().await;
            runtime.subscribe_status().await
        };

        self.inner.send_session_command(command).await?;

        tracing::info!(
            "FROST DKG command sent for {}-of-{} threshold",
            config.threshold,
            participants.len()
        );

        // Step 2: Wait for FROST DKG completion by monitoring session status
        let frost_keys = loop {
            match tokio::time::timeout(std::time::Duration::from_secs(30), status_receiver.recv())
                .await
            {
                Ok(Some(status_info)) => {
                    match status_info.status {
                        SessionStatus::Completed => {
                            tracing::info!(
                                session_id = %status_info.session_id,
                                "FROST DKG session completed successfully"
                            );
                            // For now, generate placeholder FROST keys since the runtime doesn't return them directly
                            // In a real implementation, this would be retrieved from storage or a different mechanism
                            let placeholder_frost_keys = serde_json::to_vec(&serde_json::json!({
                                "device_id": self.inner.device_id.0,
                                "threshold": config.threshold,
                                "placeholder": true
                            }))
                            .unwrap();
                            break placeholder_frost_keys;
                        }
                        SessionStatus::Failed(ref error) => {
                            return Err(crate::error::AuraError::coordination_failed(format!(
                                "FROST DKG failed: {}",
                                error
                            )));
                        }
                        _ => {
                            // Continue monitoring for completion
                            continue;
                        }
                    }
                }
                Ok(None) => {
                    return Err(crate::error::AuraError::coordination_failed(
                        "Session status channel closed",
                    ));
                }
                Err(_) => {
                    return Err(crate::error::AuraError::coordination_failed(
                        "FROST DKG timeout - no completion status received",
                    ));
                }
            }
        };

        // Validate the keys can be deserialized
        let _: aura_crypto::frost::FrostKeyShare =
            serde_json::from_slice(&frost_keys).deserialize_context("FROST keys are invalid")?;

        // Store FROST keys
        let frost_key_storage_key = crate::utils::keys::frost_keys(self.inner.device_id);
        self.inner
            .storage
            .store(&frost_key_storage_key, &frost_keys)
            .await?;

        // Step 3: Initialize key share in agent core
        // Update the key share with proper configuration
        {
            let mut key_share = self.inner.key_share.write().await;
            key_share.device_id = self.inner.device_id;
            // Store FROST keys reference
            key_share.share_data = frost_keys;
        }

        // Step 4: Initialize session runtime environment
        {
            let _session_runtime = self.inner.session_runtime.write().await;

            // TODO: Set up the session runtime environment with our ledger and transport
            // Note: set_environment method not available on LocalSessionRuntime yet
            // session_runtime.set_environment(...).await;
        }

        // Step 5: Store bootstrap metadata for audit trail
        let bootstrap_metadata = serde_json::json!({
            "timestamp": crate::utils::timestamp_millis(),
            "threshold": config.threshold,
            "share_count": config.share_count,
            "device_id": self.inner.device_id.0,
            "account_id": self.inner.account_id.0,
            "version": "phase-0",
            "parameters": config.parameters
        });

        let metadata_key = crate::utils::keys::bootstrap_metadata(self.inner.device_id);
        let metadata_bytes = serde_json::to_vec(&bootstrap_metadata)
            .serialize_context("Failed to serialize bootstrap metadata")?;

        self.inner
            .storage
            .store(&metadata_key, &metadata_bytes)
            .await
            .storage_context("Failed to store bootstrap metadata")?;

        tracing::info!(
            device_id = %self.inner.device_id,
            account_id = %self.inner.account_id,
            threshold = config.threshold,
            "Agent bootstrap completed successfully"
        );

        // Transition to idle state - agent is now ready for operations
        Ok(self.transition_to())
    }
}
