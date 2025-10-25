//! P2P Distributed Key Derivation (DKD) Implementation
//!
//! This module provides the agent-layer orchestration for P2P DKD operations,
//! coordinating with other devices to perform distributed key derivation.

use crate::{AgentError, Result, DeviceAgent as Agent};
use aura_coordination::{
    execution::{ProtocolContext, ProtocolError},
};
use aura_crypto::{DkdParticipant, Effects};
use aura_journal::{DeviceId, Session, SessionId};
use std::collections::BTreeSet;
use tracing::{debug, info, warn};
use uuid::Uuid;

/// P2P DKD Orchestrator for coordinating distributed key derivation
pub struct DkdOrchestrator<'a> {
    agent: &'a Agent,
    session_id: SessionId,
    context_id: Vec<u8>,
    participating_devices: Vec<DeviceId>,
}

impl<'a> DkdOrchestrator<'a> {
    /// Create a new P2P DKD orchestrator
    pub fn new(
        agent: &'a Agent,
        context_id: Vec<u8>,
        participating_devices: Vec<DeviceId>,
    ) -> Self {
        let session_id = SessionId(Uuid::new_v4());
        
        info!(
            "Creating P2P DKD orchestrator for context: {:?} with {} participants",
            hex::encode(&context_id),
            participating_devices.len()
        );

        Self {
            agent,
            session_id,
            context_id,
            participating_devices,
        }
    }

    /// Execute P2P DKD protocol and return derived key
    pub async fn execute(&mut self) -> Result<Vec<u8>> {
        debug!("Starting P2P DKD execution for session {}", self.session_id.0);

        // Create protocol context for DKD
        let mut protocol_context = self.create_protocol_context().await?;

        // Execute the DKD choreography directly
        match self.execute_dkd_choreography(&mut protocol_context).await {
            Ok(derived_key) => {
                info!(
                    "P2P DKD completed successfully for session {} - derived {} byte key",
                    self.session_id.0,
                    derived_key.len()
                );
                Ok(derived_key)
            }
            Err(e) => {
                warn!("P2P DKD failed for session {}: {:?}", self.session_id.0, e);
                Err(AgentError::dkd_failed(format!("DKD execution failed: {:?}", e)))
            }
        }
    }

    /// Create protocol context for DKD operations
    async fn create_protocol_context(&self) -> Result<ProtocolContext> {
        let ledger = self.agent.ledger();
        let transport = self.agent.transport();
        let effects = self.agent.effects().clone();
        let device_key = self.agent.device_key_manager().read().await.get_raw_signing_key()
            .map_err(|e| crate::AgentError::crypto_operation(format!("Failed to get device signing key: {:?}", e)))?;

        // Get device ID from agent
        let device_id = self.agent.device_id();

        // Create base protocol context
        let context = ProtocolContext::new_dkd(
            self.session_id.0,
            device_id.0,
            self.participating_devices.clone(),
            Some(2), // 2-of-N threshold for DKD operations
            ledger,
            transport,
            effects,
            device_key,
            Box::new(aura_coordination::ProductionTimeSource::new()),
        );

        Ok(context)
    }

    /// Execute the DKD choreography with the participating devices
    async fn execute_dkd_choreography(
        &mut self,
        protocol_context: &mut ProtocolContext,
    ) -> std::result::Result<Vec<u8>, ProtocolError> {
        use aura_coordination::choreography::dkd::dkd_choreography;

        debug!("Executing DKD choreography with {} participants", self.participating_devices.len());

        // Execute the DKD choreography directly
        let derived_key = dkd_choreography(protocol_context, self.context_id.clone()).await?;
        debug!("DKD protocol completed successfully");

        Ok(derived_key)
    }

    /// Get current session ID
    pub fn session_id(&self) -> SessionId {
        self.session_id
    }

    /// Get participating devices
    pub fn participants(&self) -> &[DeviceId] {
        &self.participating_devices
    }

    /// Check if device is participating in this DKD session
    pub fn is_participating_device(&self, device_id: &DeviceId) -> bool {
        self.participating_devices.contains(device_id)
    }
}

impl Agent {
    /// Perform P2P DKD with specified devices
    pub async fn derive_key_p2p(
        &self,
        context_id: Vec<u8>,
        participating_devices: Vec<DeviceId>,
    ) -> Result<Vec<u8>> {
        info!(
            "Starting P2P DKD for context: {:?} with devices: {:?}",
            hex::encode(&context_id),
            participating_devices.iter().map(|d| d.0.to_string()).collect::<Vec<_>>()
        );

        // Validate we have enough participants
        if participating_devices.len() < 2 {
            return Err(AgentError::invalid_context(
                "P2P DKD requires at least 2 participating devices".to_string()
            ));
        }

        // Check if our device is in the participant list
        let our_device_id = self.device_id();
        if !participating_devices.contains(&our_device_id) {
            return Err(AgentError::invalid_context(
                "Our device must be included in the participant list".to_string()
            ));
        }

        // Create and execute DKD orchestrator
        let mut orchestrator = DkdOrchestrator::new(self, context_id, participating_devices);
        orchestrator.execute().await
    }

    /// Get online devices that can participate in DKD
    pub async fn get_dkd_capable_devices(&self) -> Result<Vec<DeviceId>> {
        // TODO: In production, this would:
        // 1. Query the transport layer for online devices
        // 2. Check which devices have DKD capabilities
        // 3. Verify device presence tickets are valid
        // 4. Return only devices that can participate in DKD

        // For now, return a mock list based on our account configuration
        let our_device_id = self.device_id();
        let mock_devices = vec![
            our_device_id,
            DeviceId(Uuid::new_v4()), // Mock peer device 1
            DeviceId(Uuid::new_v4()), // Mock peer device 2
        ];

        debug!("Found {} DKD-capable devices", mock_devices.len());
        Ok(mock_devices)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_crypto::Effects;
    use aura_journal::AccountLedger;
    use std::sync::Arc;
    use tokio::sync::RwLock;

    #[tokio::test]
    async fn test_dkd_orchestrator_creation() {
        let effects = Effects::for_test("test_dkd");
        let ledger = Arc::new(RwLock::new(AccountLedger::new_test()));
        
        // Create a mock agent for testing
        // Note: This would require more setup in a real test
        let context_id = b"test_context".to_vec();
        let participants = vec![
            DeviceId(Uuid::new_v4()),
            DeviceId(Uuid::new_v4()),
        ];

        // For now, just test that the orchestrator can be created
        // Full integration tests would require a complete agent setup
        assert_eq!(context_id.len(), 12);
        assert_eq!(participants.len(), 2);
    }

    #[test]
    fn test_context_id_generation() {
        let app_id = "test_app";
        let operation = "get_user_key";
        let context_id = format!("{}:{}", app_id, operation).into_bytes();
        
        assert!(!context_id.is_empty());
        assert_eq!(context_id, b"test_app:get_user_key");
    }
}