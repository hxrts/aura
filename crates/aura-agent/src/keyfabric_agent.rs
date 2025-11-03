//! High-level Agent API for KeyFabric threshold operations
//!
//! This module provides a simple, high-level API for agent-driven KeyFabric operations
//! including threshold unwrapping, share contribution, and node rotation. It abstracts
//! away the choreographic complexity and provides error-resilient flows with automatic
//! participant coordination.

use crate::error::{AgentError, Result};
use crate::middleware::{AgentContext, AgentOperation};
use aura_choreography::threshold_crypto::{
    keyfabric_threshold::{
        keyfabric_threshold_unwrap, ThresholdResult,
        ThresholdUnwrapConfig,
    },
    keyfabric_share_contribution::{
        keyfabric_collect_shares, ShareCollectionResult,
        ShareContributionConfig,
    },
    keyfabric_rotation::{
        keyfabric_rotate_node, NodeRotationConfig,
        NodeRotationResult,
    },
};
use aura_protocol::effects::choreographic::ChoreographicRole;
use aura_types::effects::Effects;
use aura_types::{AccountId, DeviceId};
use rumpsteak_choreography::{ChoreoHandler, ChoreographyError};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;
use uuid::Uuid;

/// Configuration for KeyFabric agent operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyFabricAgentConfig {
    /// Default timeout for choreographic operations
    pub default_timeout_seconds: u64,
    /// Maximum number of participants allowed
    pub max_participants: usize,
    /// Default threshold for operations
    pub default_threshold: u32,
    /// Whether to enable automatic retry on failures
    pub enable_auto_retry: bool,
    /// Maximum retry attempts
    pub max_retry_attempts: u32,
    /// Whether to validate participants before starting operations
    pub validate_participants: bool,
}

impl Default for KeyFabricAgentConfig {
    fn default() -> Self {
        Self {
            default_timeout_seconds: 120, // 2 minutes
            max_participants: 10,
            default_threshold: 2,
            enable_auto_retry: true,
            max_retry_attempts: 3,
            validate_participants: true,
        }
    }

}

/// High-level parameters for threshold unwrapping
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThresholdUnwrapParams {
    /// Secret identifier to unwrap
    pub secret_id: String,
    /// Minimum shares required (M in M-of-N)
    pub threshold: u32,
    /// Total shares available (N in M-of-N)
    pub total_shares: u32,
    /// Participants in the operation
    pub participants: Vec<DeviceId>,
    /// Coordinator for the operation
    pub coordinator: DeviceId,
    /// Operation timeout (optional, uses default if None)
    pub timeout_seconds: Option<u64>,
    /// Current epoch for anti-replay protection
    pub epoch: u64,
}

/// High-level parameters for share contribution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShareContributionParams {
    /// Secret identifier for contributions
    pub secret_id: String,
    /// Minimum contributions required
    pub min_contributions: u32,
    /// Maximum contributions allowed
    pub max_contributions: u32,
    /// Participants in the operation
    pub participants: Vec<DeviceId>,
    /// Coordinator for the operation
    pub coordinator: DeviceId,
    /// Operation timeout (optional, uses default if None)
    pub timeout_seconds: Option<u64>,
    /// Current epoch for anti-replay protection
    pub epoch: u64,
}

/// High-level parameters for node rotation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeRotationParams {
    /// Node to be rotated out
    pub node_to_rotate: String,
    /// New node to rotate in
    pub new_node: String,
    /// Minimum approvals required
    pub min_approvals: u32,
    /// Participants in the operation
    pub participants: Vec<DeviceId>,
    /// Proposer for the rotation
    pub proposer: DeviceId,
    /// Operation timeout (optional, uses default if None)
    pub timeout_seconds: Option<u64>,
    /// Current epoch for anti-replay protection
    pub epoch: u64,
}

/// Result of a KeyFabric agent operation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum KeyFabricOperationResult {
    /// Threshold unwrapping completed
    ThresholdUnwrap {
        result: ThresholdResult,
        duration_ms: u64,
        participants_count: usize,
    },
    /// Share contribution completed
    ShareContribution {
        result: ShareCollectionResult,
        duration_ms: u64,
        participants_count: usize,
    },
    /// Node rotation completed
    NodeRotation {
        result: NodeRotationResult,
        duration_ms: u64,
        participants_count: usize,
    },
}

/// Session information for ongoing KeyFabric operations
#[derive(Clone)]
pub struct KeyFabricSession {
    /// Session identifier
    pub session_id: String,
    /// Account being operated on
    pub account_id: AccountId,
    /// Device performing the operation
    pub device_id: DeviceId,
    /// Current epoch
    pub epoch: u64,
    /// Session start time
    pub start_time: std::time::Instant,
    /// Participants in the session
    pub participants: Vec<ChoreographicRole>,
    /// My role in the choreography
    pub my_role: ChoreographicRole,
    /// Effects system
    pub effects: Effects,
}

impl KeyFabricSession {
    /// Create a new KeyFabric session
    pub fn new(
        account_id: AccountId,
        device_id: DeviceId,
        epoch: u64,
        participants: Vec<DeviceId>,
        effects: Effects,
    ) -> Result<Self> {
        let session_id = Uuid::new_v4().to_string();
        
        // Convert participants to choreographic roles
        let choreographic_participants: Vec<ChoreographicRole> = participants
            .iter()
            .enumerate()
            .map(|(index, &device_id)| ChoreographicRole {
                device_id: device_id.0, // Extract Uuid from DeviceId
                role_index: index,
            })
            .collect();

        // Find my role
        let my_role = *choreographic_participants
            .iter()
            .find(|role| role.device_id == device_id.0) // Extract Uuid from DeviceId
            .ok_or_else(|| AgentError::device_not_found("Device not found in participants list"))?;

        Ok(Self {
            session_id,
            account_id,
            device_id,
            epoch,
            start_time: std::time::Instant::now(),
            participants: choreographic_participants,
            my_role,
            effects,
        })
    }
    
    /// Get session duration
    pub fn duration(&self) -> Duration {
        self.start_time.elapsed()
    }
    
    /// Check if session has timed out
    pub fn is_timed_out(&self, timeout_seconds: u64) -> bool {
        self.duration().as_secs() > timeout_seconds
    }
}

/// High-level Agent API for KeyFabric operations
pub struct KeyFabricAgent {
    config: KeyFabricAgentConfig,
    active_sessions: HashMap<String, KeyFabricSession>,
}

impl KeyFabricAgent {
    /// Create a new KeyFabric agent
    pub fn new(config: KeyFabricAgentConfig) -> Self {
        Self {
            config,
            active_sessions: HashMap::new(),
        }
    }

    /// Create a new KeyFabric agent with default configuration
    pub fn with_defaults() -> Self {
        Self::new(KeyFabricAgentConfig::default())
    }

    /// Start a new KeyFabric session
    pub fn start_session(
        &mut self,
        account_id: AccountId,
        device_id: DeviceId,
        epoch: u64,
        participants: Vec<DeviceId>,
        effects: Effects,
    ) -> Result<String> {
        if participants.len() > self.config.max_participants {
            return Err(AgentError::operation_not_allowed(format!(
                "Too many participants: {} > {}",
                participants.len(),
                self.config.max_participants
            )));
        }

        if self.config.validate_participants {
            self.validate_participants(&participants)?;
        }

        let session = KeyFabricSession::new(account_id, device_id, epoch, participants, effects)?;
        let session_id = session.session_id.clone();
        
        self.active_sessions.insert(session_id.clone(), session);
        
        tracing::info!(
            session_id = %session_id,
            account_id = %account_id,
            device_id = %device_id,
            participant_count = self.active_sessions[&session_id].participants.len(),
            "KeyFabric session started"
        );
        
        Ok(session_id)
    }

    /// Execute threshold unwrapping operation
    pub async fn threshold_unwrap<H: ChoreoHandler<Role = ChoreographicRole>>(
        &self,
        session_id: &str,
        handler: &mut H,
        endpoint: &mut H::Endpoint,
        params: ThresholdUnwrapParams,
    ) -> Result<KeyFabricOperationResult> {
        let session = self.get_session(session_id)?;
        let timeout_seconds = params.timeout_seconds.unwrap_or(self.config.default_timeout_seconds);
        let start_time = std::time::Instant::now();

        if session.is_timed_out(timeout_seconds) {
            return Err(AgentError::session_timeout(format!(
                "threshold_unwrap operation timed out after {} seconds",
                timeout_seconds
            )));
        }

        // Validate parameters
        if params.threshold > params.total_shares {
            return Err(AgentError::invalid_data(format!(
                "Invalid threshold: {} > {}",
                params.threshold, params.total_shares
            )));
        }

        if params.participants.len() < params.threshold as usize {
            return Err(AgentError::invalid_data(format!(
                "Insufficient participants: {} < {}",
                params.participants.len(),
                params.threshold
            )));
        }

        // Find coordinator role
        let coordinator_role = session
            .participants
            .iter()
            .find(|role| role.device_id == params.coordinator.0) // Extract Uuid from DeviceId
            .ok_or_else(|| AgentError::device_not_found("Coordinator not found in participants"))?;

        tracing::info!(
            session_id = %session_id,
            secret_id = %params.secret_id,
            threshold = params.threshold,
            total_shares = params.total_shares,
            coordinator = ?coordinator_role,
            "Starting threshold unwrap operation"
        );

        // Create threshold unwrap configuration
        let config = ThresholdUnwrapConfig {
            threshold: params.threshold,
            total_shares: params.total_shares,
            epoch: params.epoch,
            timeout_seconds,
            secret_id: params.secret_id,
        };

        // Execute the choreography
        let result = keyfabric_threshold_unwrap(
            handler,
            endpoint,
            session.participants.clone(),
            session.my_role,
            *coordinator_role,
            config,
            session.effects.clone(),
        )
        .await
        .map_err(|e| AgentError::coordination_failed(format!("Choreography failed: {}", e)))?;

        let duration = start_time.elapsed();

        tracing::info!(
            session_id = %session_id,
            secret_id = %result.secret_id,
            shares_used = result.shares_used,
            duration_ms = duration.as_millis(),
            "Threshold unwrap operation completed successfully"
        );

        Ok(KeyFabricOperationResult::ThresholdUnwrap {
            result,
            duration_ms: duration.as_millis() as u64,
            participants_count: session.participants.len(),
        })
    }

    /// Execute share contribution operation
    pub async fn share_contribution<H: ChoreoHandler<Role = ChoreographicRole>>(
        &self,
        session_id: &str,
        handler: &mut H,
        endpoint: &mut H::Endpoint,
        params: ShareContributionParams,
    ) -> Result<KeyFabricOperationResult> {
        let session = self.get_session(session_id)?;
        let timeout_seconds = params.timeout_seconds.unwrap_or(self.config.default_timeout_seconds);
        let start_time = std::time::Instant::now();

        if session.is_timed_out(timeout_seconds) {
            return Err(AgentError::session_timeout(format!(
                "share_contribution operation timed out after {} seconds",
                timeout_seconds
            )));
        }

        // Validate parameters
        if params.min_contributions > params.max_contributions {
            return Err(AgentError::invalid_data(format!(
                "Invalid contribution bounds: {} > {}",
                params.min_contributions, params.max_contributions
            )));
        }

        if params.participants.len() < params.min_contributions as usize {
            return Err(AgentError::invalid_data(format!(
                "Insufficient participants: {} < {}",
                params.participants.len(),
                params.min_contributions
            )));
        }

        // Find coordinator role
        let coordinator_role = session
            .participants
            .iter()
            .find(|role| role.device_id == params.coordinator.0) // Extract Uuid from DeviceId
            .ok_or_else(|| AgentError::device_not_found("Coordinator not found in participants"))?;

        tracing::info!(
            session_id = %session_id,
            secret_id = %params.secret_id,
            min_contributions = params.min_contributions,
            max_contributions = params.max_contributions,
            coordinator = ?coordinator_role,
            "Starting share contribution operation"
        );

        // Create share contribution configuration
        let config = ShareContributionConfig {
            min_contributions: params.min_contributions,
            max_contributions: params.max_contributions,
            secret_id: params.secret_id,
            epoch: params.epoch,
            timeout_seconds,
        };

        // Execute the choreography
        let result = keyfabric_collect_shares(
            handler,
            endpoint,
            session.participants.clone(),
            session.my_role,
            *coordinator_role,
            config,
            session.effects.clone(),
        )
        .await
        .map_err(|e| AgentError::coordination_failed(format!("Choreography failed: {}", e)))?;

        let duration = start_time.elapsed();

        tracing::info!(
            session_id = %session_id,
            secret_id = %result.secret_id,
            shares_collected = result.shares_collected,
            collection_complete = result.collection_complete,
            duration_ms = duration.as_millis(),
            "Share contribution operation completed successfully"
        );

        Ok(KeyFabricOperationResult::ShareContribution {
            result,
            duration_ms: duration.as_millis() as u64,
            participants_count: session.participants.len(),
        })
    }

    /// Execute node rotation operation
    pub async fn node_rotation<H: ChoreoHandler<Role = ChoreographicRole>>(
        &self,
        session_id: &str,
        handler: &mut H,
        endpoint: &mut H::Endpoint,
        params: NodeRotationParams,
    ) -> Result<KeyFabricOperationResult> {
        let session = self.get_session(session_id)?;
        let timeout_seconds = params.timeout_seconds.unwrap_or(self.config.default_timeout_seconds);
        let start_time = std::time::Instant::now();

        if session.is_timed_out(timeout_seconds) {
            return Err(AgentError::session_timeout(format!(
                "node_rotation operation timed out after {} seconds",
                timeout_seconds
            )));
        }

        // Validate parameters
        if params.min_approvals > params.participants.len() as u32 {
            return Err(AgentError::invalid_data(format!(
                "Too many approvals required: {} > {}",
                params.min_approvals,
                params.participants.len()
            )));
        }

        // Find proposer role
        let proposer_role = session
            .participants
            .iter()
            .find(|role| role.device_id == params.proposer.0) // Extract Uuid from DeviceId
            .ok_or_else(|| AgentError::device_not_found("Proposer not found in participants"))?;

        tracing::info!(
            session_id = %session_id,
            node_to_rotate = %params.node_to_rotate,
            new_node = %params.new_node,
            min_approvals = params.min_approvals,
            proposer = ?proposer_role,
            "Starting node rotation operation"
        );

        // Create node rotation configuration
        let config = NodeRotationConfig {
            node_to_rotate: params.node_to_rotate,
            new_node: params.new_node,
            min_approvals: params.min_approvals,
            epoch: params.epoch,
            timeout_seconds,
        };

        // Execute the choreography
        let result = keyfabric_rotate_node(
            handler,
            endpoint,
            session.participants.clone(),
            session.my_role,
            *proposer_role,
            config,
            session.effects.clone(),
        )
        .await
        .map_err(|e| AgentError::coordination_failed(format!("Choreography failed: {}", e)))?;

        let duration = start_time.elapsed();

        tracing::info!(
            session_id = %session_id,
            rotation_approved = result.rotation_approved,
            rotation_completed = result.rotation_completed,
            approvals_count = result.approvals.len(),
            duration_ms = duration.as_millis(),
            "Node rotation operation completed successfully"
        );

        Ok(KeyFabricOperationResult::NodeRotation {
            result,
            duration_ms: duration.as_millis() as u64,
            participants_count: session.participants.len(),
        })
    }

    /// End a KeyFabric session
    pub fn end_session(&mut self, session_id: &str) -> Result<Duration> {
        let session = self.active_sessions.remove(session_id).ok_or_else(|| {
            AgentError::session_not_found(format!("Session not found: {}", session_id))
        })?;

        let duration = session.duration();
        
        tracing::info!(
            session_id = %session_id,
            account_id = %session.account_id,
            device_id = %session.device_id,
            duration_ms = duration.as_millis(),
            "KeyFabric session ended"
        );
        
        Ok(duration)
    }

    /// Get session information
    pub fn get_session(&self, session_id: &str) -> Result<&KeyFabricSession> {
        self.active_sessions.get(session_id).ok_or_else(|| {
            AgentError::session_not_found(format!("Session not found: {}", session_id))
        })
    }

    /// List active sessions
    pub fn list_sessions(&self) -> Vec<String> {
        self.active_sessions.keys().cloned().collect()
    }

    /// Clean up timed out sessions
    pub fn cleanup_timed_out_sessions(&mut self) {
        let timeout_seconds = self.config.default_timeout_seconds;
        let timed_out_sessions: Vec<String> = self
            .active_sessions
            .iter()
            .filter_map(|(session_id, session)| {
                if session.is_timed_out(timeout_seconds) {
                    Some(session_id.clone())
                } else {
                    None
                }
            })
            .collect();

        for session_id in timed_out_sessions {
            if let Some(session) = self.active_sessions.remove(&session_id) {
                tracing::warn!(
                    session_id = %session_id,
                    duration_ms = session.duration().as_millis(),
                    timeout_seconds = timeout_seconds,
                    "Session timed out and cleaned up"
                );
            }
        }
    }

    // Private helper methods

    /// Validate participants before starting operations
    fn validate_participants(&self, participants: &[DeviceId]) -> Result<()> {
        if participants.is_empty() {
            return Err(AgentError::invalid_data("Participants list cannot be empty"));
        }

        // Check for duplicate participants
        let mut seen = std::collections::HashSet::new();
        for participant in participants {
            if !seen.insert(participant) {
                return Err(AgentError::invalid_data(format!("Duplicate participant: {}", participant)));
            }
        }

        Ok(())
    }

    /// Execute operation with retry logic
    async fn execute_with_retry<F, Fut, T>(&self, mut operation: F, operation_name: &str) -> Result<T>
    where
        F: FnMut() -> Fut,
        Fut: std::future::Future<Output = std::result::Result<T, ChoreographyError>>,
    {
        let mut attempt = 0;
        let mut last_error = None;

        while attempt < self.config.max_retry_attempts {
            attempt += 1;

            match operation().await {
                Ok(result) => {
                    if attempt > 1 {
                        tracing::info!(
                            operation = operation_name,
                            attempt = attempt,
                            "Operation succeeded after retry"
                        );
                    }
                    return Ok(result);
                }
                Err(e) => {
                    last_error = Some(e);
                    if attempt < self.config.max_retry_attempts {
                        tracing::warn!(
                            operation = operation_name,
                            attempt = attempt,
                            max_attempts = self.config.max_retry_attempts,
                            error = %last_error.as_ref().unwrap(),
                            "Operation failed, retrying"
                        );
                        
                        // Exponential backoff
                        let delay_ms = 1000 * (1u64 << (attempt - 1));
                        tokio::time::sleep(Duration::from_millis(delay_ms)).await;
                    }
                }
            }
        }

        Err(AgentError::coordination_failed(format!(
            "Operation failed after {} attempts: {}",
            self.config.max_retry_attempts,
            last_error.unwrap()
        )))
    }
}

/// Create a new KeyFabric agent context
pub fn create_keyfabric_context(
    account_id: AccountId,
    device_id: DeviceId,
    operation_type: String,
) -> AgentContext {
    AgentContext::new(account_id, device_id, operation_type)
        .with_metadata("agent_type".to_string(), "keyfabric".to_string())
        .with_metadata("api_version".to_string(), "1.0".to_string())
}

/// Convenience function to create threshold unwrap operation
pub fn create_threshold_unwrap_operation(
    secret_id: String,
    threshold: u32,
    total_shares: u32,
) -> AgentOperation {
    AgentOperation::DeriveIdentity {
        app_id: format!("keyfabric_threshold_{}", secret_id),
        context: format!("threshold_{}_{}", threshold, total_shares),
    }
}

/// Convenience function to create share contribution operation
pub fn create_share_contribution_operation(
    secret_id: String,
    min_contributions: u32,
    max_contributions: u32,
) -> AgentOperation {
    AgentOperation::StartSession {
        session_type: format!("keyfabric_share_contribution_{}", secret_id),
        participants: vec![], // Will be filled by the agent
    }
}

/// Convenience function to create node rotation operation
pub fn create_node_rotation_operation(
    node_to_rotate: String,
    new_node: String,
    min_approvals: u32,
) -> AgentOperation {
    AgentOperation::InitiateBackup {
        backup_type: format!("keyfabric_rotation_{}_{}", node_to_rotate, new_node),
        guardians: vec![], // Will be filled by the agent
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_types::effects::Effects;

    #[tokio::test]
    async fn test_keyfabric_agent_creation() {
        let config = KeyFabricAgentConfig::default();
        let agent = KeyFabricAgent::new(config);

        assert_eq!(agent.config.default_timeout_seconds, 120);
        assert_eq!(agent.config.max_participants, 10);
        assert_eq!(agent.config.default_threshold, 2);
        assert!(agent.config.enable_auto_retry);
        assert_eq!(agent.config.max_retry_attempts, 3);
        assert!(agent.config.validate_participants);
        assert!(agent.active_sessions.is_empty());
    }

    #[tokio::test]
    async fn test_session_management() {
        let mut agent = KeyFabricAgent::with_defaults();
        let effects = Effects::test(42);
        
        let account_id = Uuid::new_v4();
        let device_id = Uuid::new_v4();
        let epoch = 1;
        let participants = vec![device_id, Uuid::new_v4(), Uuid::new_v4()];

        // Start session
        let session_id = agent
            .start_session(account_id, device_id, epoch, participants.clone(), effects)
            .unwrap();

        assert!(!session_id.is_empty());
        assert_eq!(agent.list_sessions().len(), 1);
        assert!(agent.list_sessions().contains(&session_id));

        // Get session
        let session = agent.get_session(&session_id).unwrap();
        assert_eq!(session.account_id, account_id);
        assert_eq!(session.device_id, device_id);
        assert_eq!(session.epoch, epoch);
        assert_eq!(session.participants.len(), participants.len());

        // End session
        let duration = agent.end_session(&session_id).unwrap();
        assert!(duration.as_millis() > 0);
        assert_eq!(agent.list_sessions().len(), 0);
    }

    #[test]
    fn test_parameter_validation() {
        let params = ThresholdUnwrapParams {
            secret_id: "test_secret".to_string(),
            threshold: 3,
            total_shares: 2, // Invalid: threshold > total_shares
            participants: vec![Uuid::new_v4(), Uuid::new_v4()],
            coordinator: Uuid::new_v4(),
            timeout_seconds: None,
            epoch: 1,
        };

        // This should be caught during operation execution
        assert!(params.threshold > params.total_shares);
    }

    #[test]
    fn test_config_defaults() {
        let config = KeyFabricAgentConfig::default();
        
        assert_eq!(config.default_timeout_seconds, 120);
        assert_eq!(config.max_participants, 10);
        assert_eq!(config.default_threshold, 2);
        assert!(config.enable_auto_retry);
        assert_eq!(config.max_retry_attempts, 3);
        assert!(config.validate_participants);
    }

    #[test]
    fn test_convenience_functions() {
        let account_id = Uuid::new_v4();
        let device_id = Uuid::new_v4();
        
        let context = create_keyfabric_context(
            account_id,
            device_id,
            "test_operation".to_string(),
        );
        
        assert_eq!(context.account_id, account_id);
        assert_eq!(context.device_id, device_id);
        assert_eq!(context.operation_type, "test_operation");
        assert_eq!(context.metadata.get("agent_type"), Some(&"keyfabric".to_string()));
        assert_eq!(context.metadata.get("api_version"), Some(&"1.0".to_string()));
    }
}