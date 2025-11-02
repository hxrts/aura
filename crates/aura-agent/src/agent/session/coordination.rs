//! Recovery and resharing protocol coordination
//!
//! This module provides methods for initiating long-running coordination
//! protocols that transition the agent to the Coordinating state.

use super::states::{AgentProtocol, Coordinating, Idle};
use crate::{Result, Storage};
use aura_types::DeviceId;
use std::collections::HashMap;

impl<S: Storage> AgentProtocol<S, Idle> {
    /// Initiate a recovery protocol
    ///
    /// This consumes the idle agent and returns a coordinating agent
    pub async fn initiate_recovery(
        self,
        recovery_params: serde_json::Value,
    ) -> Result<AgentProtocol<S, Coordinating>> {
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

        // Create metadata for recovery protocol
        let metadata = [
            ("guardian_threshold".to_string(), guardian_threshold.to_string()),
            ("cooldown_seconds".to_string(), cooldown_seconds.to_string()),
        ].into_iter().collect();

        // Start recovery protocol session
        let _session_id = self.inner.start_protocol_session(
            "recovery",
            vec![], // participants determined by protocol
            metadata,
        ).await?;

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
    ) -> Result<AgentProtocol<S, Coordinating>> {
        tracing::info!(
            device_id = %self.inner.device_id,
            new_threshold = new_threshold,
            new_participants = ?new_participants,
            "Initiating resharing protocol"
        );

        // Create metadata for resharing protocol
        let mut metadata = HashMap::new();
        metadata.insert("new_threshold".to_string(), new_threshold.to_string());
        metadata.insert("new_participants".to_string(), 
            serde_json::to_string(&new_participants).unwrap_or_default());

        // Start resharing protocol session
        let _session_id = self.inner.start_protocol_session(
            "resharing",
            new_participants.clone(),
            metadata,
        ).await?;

        // Transition to coordinating state
        Ok(self.transition_to())
    }
}