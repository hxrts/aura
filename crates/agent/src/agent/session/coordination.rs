//! Recovery and resharing protocol coordination
//!
//! This module provides methods for initiating long-running coordination
//! protocols that transition the agent to the Coordinating state.

use super::states::{AgentProtocol, Coordinating, Idle};
use crate::{Result, Storage, Transport};
use aura_types::DeviceId;

impl<T: Transport, S: Storage> AgentProtocol<T, S, Idle> {
    /// Initiate a recovery protocol
    ///
    /// This consumes the idle agent and returns a coordinating agent
    pub async fn initiate_recovery(
        self,
        recovery_params: serde_json::Value,
    ) -> Result<AgentProtocol<T, S, Coordinating>> {
        tracing::info!(
            device_id = %self.inner.device_id,
            "Initiating recovery protocol"
        );

        // Extract recovery parameters
        let guardian_threshold = recovery_params
            .get("guardian_threshold")
            .and_then(|v| v.as_u64())
            .unwrap_or(2) as usize;
        let cooldown_seconds = recovery_params
            .get("cooldown_seconds")
            .and_then(|v| v.as_u64())
            .unwrap_or(300); // 5 minutes default

        // Send recovery command using the new API
        let command = aura_protocol::SessionCommand::StartRecovery {
            guardian_threshold,
            cooldown_seconds,
        };

        self.inner.send_session_command(command).await?;

        // Transition to coordinating state
        Ok(self.transition_to())
    }

    /// Initiate a resharing protocol
    ///
    /// This consumes the idle agent and returns a coordinating agent
    pub async fn initiate_resharing(
        self,
        new_threshold: u16,
        new_participants: Vec<DeviceId>,
    ) -> Result<AgentProtocol<T, S, Coordinating>> {
        tracing::info!(
            device_id = %self.inner.device_id,
            new_threshold = new_threshold,
            new_participants = ?new_participants,
            "Initiating resharing protocol"
        );

        // Send resharing command using the new API
        let command = aura_protocol::SessionCommand::StartResharing {
            new_participants,
            new_threshold: new_threshold as usize,
        };

        self.inner.send_session_command(command).await?;

        // Transition to coordinating state
        Ok(self.transition_to())
    }
}
