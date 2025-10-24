//! Session-typed DeviceAgent implementation
//!
//! This module provides a session-typed wrapper around the existing DeviceAgent
//! that integrates with the LocalSessionRuntime for type-safe protocol coordination.

use crate::{
    AgentError, ContextCapsule, DerivedIdentity, IdentityConfig, Result, SessionCredential,
};
use aura_coordination::{
    LocalSessionRuntime, SessionCommand, SessionEffect, SessionEvent, SessionProtocolType,
};
use aura_session_types::{
    new_session_typed_agent, AgentIdle, AgentRecoveryCompleted, AgentSessionState, DkdCompleted,
    DkdInProgress, ProtocolFailed, RecoveryInProgress, SessionTypedAgent,
};
use aura_crypto::Effects;
use aura_journal::{AccountId, AccountLedger, DeviceId};
use std::ops::Deref;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

/// Session-typed DeviceAgent that provides type-safe protocol coordination
///
/// This agent integrates with the LocalSessionRuntime to ensure that all protocol
/// operations follow session type guarantees while maintaining backward compatibility
/// with the existing DeviceAgent API.
pub struct SessionTypedDeviceAgent {
    /// Device configuration
    config: IdentityConfig,
    /// Local session runtime for protocol coordination
    runtime: Arc<LocalSessionRuntime>,
    /// Current agent session state
    agent_session: Arc<RwLock<AgentSessionState>>,
    /// Account ledger
    ledger: Arc<RwLock<AccountLedger>>,
    /// Effects for deterministic operations
    effects: Effects,
}

impl SessionTypedDeviceAgent {
    /// Create a new session-typed device agent
    pub async fn new(
        config: IdentityConfig,
        ledger: AccountLedger,
        effects: Effects,
    ) -> Result<Self> {
        info!(
            "Initializing SessionTypedDeviceAgent for device {}",
            config.device_id
        );

        // Create local session runtime
        let runtime = Arc::new(LocalSessionRuntime::new(
            config.device_id,
            config.account_id,
        ));

        // Create initial agent session in idle state
        let agent_session = new_session_typed_agent(config.device_id);

        let agent = Self {
            config,
            runtime,
            agent_session: Arc::new(RwLock::new(AgentSessionState::AgentIdle(agent_session))),
            ledger: Arc::new(RwLock::new(ledger)),
            effects,
        };

        // Start the session runtime
        tokio::spawn({
            let runtime = agent.runtime.clone();
            async move {
                if let Err(e) = runtime.run().await {
                    error!("Session runtime error: {}", e);
                }
            }
        });

        info!("SessionTypedDeviceAgent initialized successfully");
        Ok(agent)
    }

    /// Derive a simple identity using session-typed DKD protocol
    pub async fn derive_simple_identity(
        &self,
        app_id: &str,
        context_label: &str,
    ) -> Result<(DerivedIdentity, SessionCredential)> {
        debug!(
            "Deriving simple identity for app={}, context={} using session types",
            app_id, context_label
        );

        // Check current agent state
        let current_state = {
            let session = self.agent_session.read().await;
            session.state_name().to_string()
        };

        // Can only derive identity from AgentIdle state
        if current_state != "AgentIdle" {
            return Err(AgentError::InvalidContext(format!(
                "Cannot derive identity in state {}, must be in AgentIdle",
                current_state
            )));
        }

        // Start DKD session
        let dkd_session_id = self
            .runtime
            .start_dkd_session(app_id.to_string(), context_label.to_string())
            .await
            .map_err(|e| {
                AgentError::OrchestratorError(format!("Failed to start DKD session: {}", e))
            })?;

        // Transition agent to DkdInProgress state
        {
            let mut session = self.agent_session.write().await;
            // Extract the inner protocol and transition it
            if let AgentSessionState::AgentIdle(protocol) = session.deref() {
                let transitioned = protocol.clone().transition_to::<DkdInProgress>();
                *session = AgentSessionState::DkdInProgress(transitioned);
            }
        }

        info!(
            "Started DKD protocol session {} for identity derivation",
            dkd_session_id
        );

        // TODO: Implement actual DKD coordination with peers
        // For now, create a placeholder derived identity
        let capsule = ContextCapsule::simple_with_effects(app_id, context_label, &self.effects)?;

        // Simulate DKD completion
        let derived_identity = self.simulate_dkd_completion(&capsule).await?;

        // Issue session credential
        let session_credential = self.issue_session_credential(&derived_identity).await?;

        // Transition agent back to idle state with DKD completion witness
        {
            let mut session = self.agent_session.write().await;
            // Extract the inner protocol and transition it
            if let AgentSessionState::DkdInProgress(protocol) = session.deref() {
                let transitioned = protocol.clone().transition_to::<AgentIdle>();
                *session = AgentSessionState::AgentIdle(transitioned);
            }
        }

        // Terminate DKD session
        self.runtime
            .terminate_session(dkd_session_id)
            .await
            .map_err(|e| {
                AgentError::OrchestratorError(format!("Failed to terminate DKD session: {}", e))
            })?;

        info!("DKD protocol completed successfully");
        Ok((derived_identity, session_credential))
    }

    /// Initiate recovery protocol using session types
    pub async fn initiate_recovery(&self, guardian_threshold: u16) -> Result<Uuid> {
        debug!(
            "Initiating recovery protocol with threshold {}",
            guardian_threshold
        );

        // Check current agent state
        let current_state = {
            let session = self.agent_session.read().await;
            session.state_name().to_string()
        };

        // Can only initiate recovery from AgentIdle state
        if current_state != "AgentIdle" {
            return Err(AgentError::InvalidContext(format!(
                "Cannot initiate recovery in state {}, must be in AgentIdle",
                current_state
            )));
        }

        // Start recovery session
        let recovery_session_id = self
            .runtime
            .start_recovery_session(
                guardian_threshold as usize,
                48 * 3600, // 48 hour cooldown
            )
            .await
            .map_err(|e| {
                AgentError::OrchestratorError(format!("Failed to start recovery session: {}", e))
            })?;

        // Transition agent to RecoveryInProgress state
        {
            let mut session = self.agent_session.write().await;
            // Extract the inner protocol and transition it
            if let AgentSessionState::AgentIdle(protocol) = session.deref() {
                let transitioned = protocol.clone().transition_to::<RecoveryInProgress>();
                *session = AgentSessionState::RecoveryInProgress(transitioned);
            }
        }

        info!("Started recovery protocol session {}", recovery_session_id);
        Ok(recovery_session_id)
    }

    /// Get current agent session state
    pub async fn get_current_state(&self) -> String {
        let session = self.agent_session.read().await;
        session.state_name().to_string()
    }

    /// Check if agent can be safely terminated
    pub async fn can_terminate(&self) -> bool {
        let session = self.agent_session.read().await;
        session.can_terminate()
    }

    /// Get session statistics from the runtime
    pub async fn get_session_statistics(&self) -> Result<crate::SessionStatistics> {
        let status = self.runtime.get_session_status().await;

        let total_sessions = status.len();
        let active_sessions = status.iter().filter(|s| !s.is_final).count();
        let completed_sessions = status.iter().filter(|s| s.is_final).count();
        let failed_sessions = 0; // TODO: Track failed sessions
        let timed_out_sessions = 0; // TODO: Track timed out sessions

        let mut sessions_by_protocol = std::collections::BTreeMap::new();
        for session in &status {
            *sessions_by_protocol
                .entry(map_protocol_type(&session.protocol_type))
                .or_insert(0) += 1;
        }

        Ok(crate::SessionStatistics {
            total_sessions,
            active_sessions,
            completed_sessions,
            failed_sessions,
            timed_out_sessions,
            sessions_by_protocol,
        })
    }

    /// Send a command to the session runtime
    pub async fn send_session_command(&self, command: SessionCommand) -> Result<()> {
        self.runtime.send_command(command).await.map_err(|e| {
            AgentError::OrchestratorError(format!("Failed to send session command: {}", e))
        })
    }

    /// Get the underlying session runtime for advanced operations
    pub fn runtime(&self) -> &LocalSessionRuntime {
        &self.runtime
    }

    // Private helper methods

    /// Simulate DKD completion for testing (placeholder implementation)
    async fn simulate_dkd_completion(&self, capsule: &ContextCapsule) -> Result<DerivedIdentity> {
        // TODO: Replace with actual P2P DKD coordination
        warn!("Using simulated DKD completion - replace with real P2P coordination");

        // Generate placeholder derived identity
        let signing_key = ed25519_dalek::SigningKey::from_bytes(&rand::random());
        let pk_derived = signing_key.verifying_key();
        let seed_fingerprint = [42u8; 32]; // Placeholder

        Ok(DerivedIdentity {
            capsule: capsule.clone(),
            pk_derived,
            seed_fingerprint,
        })
    }

    /// Issue a session credential for a derived identity
    async fn issue_session_credential(
        &self,
        identity: &DerivedIdentity,
    ) -> Result<SessionCredential> {
        let ledger = self.ledger.read().await;
        let session_epoch = ledger.state().session_epoch;
        drop(ledger);

        // Create challenge and issue credential
        let challenge = self.effects.random_bytes();
        let nonce = self.effects.now()?;

        crate::credential::issue_credential(
            self.config.device_id,
            session_epoch,
            identity,
            &challenge,
            "identity:derived",
            nonce,
            None,            // No device attestation for now
            Some(24 * 3600), // 24 hour TTL
            &self.effects,
        )
    }
}

/// Map SessionProtocolType to aura_journal::ProtocolType
fn map_protocol_type(protocol_type: &SessionProtocolType) -> aura_journal::ProtocolType {
    match protocol_type {
        SessionProtocolType::DKD => aura_journal::ProtocolType::Dkd,
        SessionProtocolType::Recovery => aura_journal::ProtocolType::Recovery,
        SessionProtocolType::Resharing => aura_journal::ProtocolType::Resharing,
        SessionProtocolType::Locking => aura_journal::ProtocolType::Locking,
        SessionProtocolType::Agent => aura_journal::ProtocolType::LockAcquisition,
    }
}

/// Backward compatibility wrapper that maintains the original DeviceAgent API
/// while using session types internally
pub struct DeviceAgentCompat {
    session_agent: SessionTypedDeviceAgent,
}

impl DeviceAgentCompat {
    /// Create a new backward-compatible DeviceAgent
    pub async fn new(
        config: IdentityConfig,
        ledger: AccountLedger,
        effects: Effects,
    ) -> Result<Self> {
        let session_agent = SessionTypedDeviceAgent::new(config, ledger, effects).await?;
        Ok(Self { session_agent })
    }

    /// Maintain backward compatibility with original derive_simple_identity API
    pub async fn derive_simple_identity(
        &self,
        app_id: &str,
        context_label: &str,
    ) -> Result<(DerivedIdentity, SessionCredential)> {
        self.session_agent
            .derive_simple_identity(app_id, context_label)
            .await
    }

    /// Maintain backward compatibility with original initiate_recovery API
    pub async fn initiate_recovery(&self, guardian_threshold: u16) -> Result<Uuid> {
        self.session_agent
            .initiate_recovery(guardian_threshold)
            .await
    }

    /// Get session statistics (enhanced with session type information)
    pub async fn get_session_statistics(&self) -> Result<crate::SessionStatistics> {
        self.session_agent.get_session_statistics().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_journal::{AccountState, DeviceMetadata};
    use std::collections::BTreeMap;

    async fn create_test_agent() -> SessionTypedDeviceAgent {
        let effects = Effects::test();
        let device_id = DeviceId::new_with_effects(&effects);
        let account_id = AccountId::new_with_effects(&effects);

        let config = IdentityConfig {
            device_id,
            account_id,
            participant_id: aura_coordination::ParticipantId::new(1),
            share_path: "/tmp/test_share".to_string(),
            threshold: 2,
            total_participants: 3,
        };

        // Create test ledger
        let mut devices = BTreeMap::new();
        devices.insert(
            device_id,
            DeviceMetadata {
                public_key: ed25519_dalek::SigningKey::from_bytes(&[1u8; 32]).verifying_key(),
                device_attestation: None,
                added_at: 0,
                capabilities: Vec::new(),
            },
        );

        let account_state = AccountState {
            account_id,
            devices,
            session_epoch: aura_journal::SessionEpoch::initial(),
            guardian_config: None,
        };

        let ledger = AccountLedger::new(account_state).unwrap();

        SessionTypedDeviceAgent::new(config, ledger, effects)
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn test_session_agent_creation() {
        let agent = create_test_agent().await;

        // Should start in AgentIdle state
        assert_eq!(agent.get_current_state().await, "AgentIdle");
        assert!(agent.can_terminate().await);
    }

    #[tokio::test]
    async fn test_derive_identity_state_transitions() {
        let agent = create_test_agent().await;

        // Should start in AgentIdle
        assert_eq!(agent.get_current_state().await, "AgentIdle");

        // Derive identity should work from idle state
        let result = agent
            .derive_simple_identity("test-app", "test-context")
            .await;
        assert!(result.is_ok());

        // Should return to AgentIdle state after completion
        assert_eq!(agent.get_current_state().await, "AgentIdle");
    }

    #[tokio::test]
    async fn test_recovery_initiation() {
        let agent = create_test_agent().await;

        // Should start in AgentIdle
        assert_eq!(agent.get_current_state().await, "AgentIdle");

        // Initiate recovery
        let recovery_id = agent.initiate_recovery(2).await.unwrap();
        assert!(recovery_id != Uuid::nil());

        // Should be in RecoveryInProgress state
        assert_eq!(agent.get_current_state().await, "RecoveryInProgress");
        assert!(!agent.can_terminate().await); // Busy with recovery
    }

    #[tokio::test]
    async fn test_session_statistics() {
        let agent = create_test_agent().await;

        let stats = agent.get_session_statistics().await.unwrap();

        // Should have at least the agent session
        assert!(stats.total_sessions >= 1);
        assert!(stats.active_sessions >= 1);
    }

    #[tokio::test]
    async fn test_backward_compatibility() {
        let effects = Effects::test();
        let device_id = DeviceId::new_with_effects(&effects);
        let account_id = AccountId::new_with_effects(&effects);

        let config = IdentityConfig {
            device_id,
            account_id,
            participant_id: aura_coordination::ParticipantId::new(1),
            share_path: "/tmp/test_share".to_string(),
            threshold: 2,
            total_participants: 3,
        };

        let mut devices = BTreeMap::new();
        devices.insert(
            device_id,
            DeviceMetadata {
                public_key: ed25519_dalek::SigningKey::from_bytes(&[1u8; 32]).verifying_key(),
                device_attestation: None,
                added_at: 0,
                capabilities: Vec::new(),
            },
        );

        let account_state = AccountState {
            account_id,
            devices,
            session_epoch: aura_journal::SessionEpoch::initial(),
            guardian_config: None,
        };

        let ledger = AccountLedger::new(account_state).unwrap();

        // Create backward-compatible agent
        let compat_agent = DeviceAgentCompat::new(config, ledger, effects)
            .await
            .unwrap();

        // Should work with original API
        let result = compat_agent
            .derive_simple_identity("test-app", "test-context")
            .await;
        assert!(result.is_ok());
    }
}
