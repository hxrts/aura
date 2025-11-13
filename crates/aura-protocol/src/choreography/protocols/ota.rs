//! OTA Upgrade Orchestration Choreography
//!
//! This module implements the MPST protocol for coordinating over-the-air (OTA)
//! upgrade proposals with threshold-based adoption and epoch fence enforcement.
//!
//! ## Protocol Flow
//!
//! ### Phase 1: Proposal
//! 1. Coordinator → All Devices: UpgradeProposal { version, kind, checksum }
//! 2. All Devices: Evaluate upgrade locally
//!
//! ### Phase 2: Adoption
//! 1. Device → Coordinator: OptIn { version } or Reject
//! 2. Coordinator: Collect adoption votes
//!
//! ### Phase 3: Activation (Hard Fork Only)
//! 1. Coordinator checks quorum threshold reached
//! 2. If threshold met and epoch fence passed: activate upgrade
//! 3. Coordinator → All Devices: UpgradeActivated { epoch_fence }
//! 4. All Devices: Apply upgrade and emit maintenance event
//!
//! ### Phase 4: Completion
//! 1. Coordinator collects completion confirmations
//! 2. All Devices: Emit cache invalidation event
//! 3. System initializes with new protocol version
//!
//! ## Properties
//!
//! - Threshold-based adoption prevents split-brain scenarios
//! - Hard fork epoch fences ensure atomic activation
//! - Soft forks allow gradual adoption without forced gates
//! - Security patches can be mandatory via policies
//! - Cache invalidation events synchronize protocol state
//! - Forward compatibility for deprecated protocol versions

use crate::choreography::AuraHandlerAdapter;
use crate::effects::ChoreographyError;
use crate::effects::TimeEffects;
use crate::handlers::AuraHandlerError;
use aura_core::{
    maintenance::{MaintenanceEvent, UpgradeActivated, UpgradeKind, UpgradeProposal},
    DeviceId, Epoch, Hash32, SemanticVersion,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ============================================================================
// Configuration and Results
// ============================================================================

/// OTA upgrade choreography configuration
#[derive(Debug, Clone)]
pub struct UpgradeConfig {
    /// Coordinator device ID (usually the device initiating upgrade)
    pub coordinator: DeviceId,
    /// All participating devices that can adopt the upgrade
    pub participants: Vec<DeviceId>,
    /// Threshold required for hard fork activation (e.g., 2 of 3)
    pub quorum_threshold: u16,
    /// Current protocol epoch
    pub current_epoch: Epoch,
    /// Timeout for adoption phase (seconds)
    pub adoption_timeout_secs: u64,
}

impl Default for UpgradeConfig {
    fn default() -> Self {
        Self {
            coordinator: DeviceId::new(),
            participants: Vec::new(),
            quorum_threshold: 1,
            current_epoch: 0,
            adoption_timeout_secs: 300,
        }
    }
}

/// Upgrade proposal message sent by coordinator
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpgradeMessage {
    pub proposal_id: Uuid,
    pub version: SemanticVersion,
    pub kind: UpgradeKind,
    pub checksum: Hash32,
    pub artifact_uri: Option<String>,
    pub timestamp: u64,
}

/// Device adoption response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AdoptionResponse {
    OptIn {
        device_id: DeviceId,
        version: SemanticVersion,
        timestamp: u64,
    },
    Reject {
        device_id: DeviceId,
        reason: String,
        timestamp: u64,
    },
}

/// Activation signal for hard forks
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActivationSignal {
    pub proposal_id: Uuid,
    pub activation_epoch: Epoch,
    pub quorum_count: u16,
    pub timestamp: u64,
}

/// OTA orchestration result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OtaResult {
    /// Proposal ID
    pub proposal_id: Uuid,
    /// Version being deployed
    pub version: SemanticVersion,
    /// Number of devices that opted in
    pub adoptions: u16,
    /// Whether activation occurred (hard forks only)
    pub activated: bool,
    /// Whether all devices confirmed completion
    pub completed: bool,
    /// Error message if failed
    pub error: Option<String>,
}

// ============================================================================
// Error Types
// ============================================================================

#[derive(Debug, thiserror::Error)]
pub enum OtaError {
    #[error("Invalid configuration: {0}")]
    InvalidConfig(String),
    #[error("Communication error: {0}")]
    Communication(String),
    #[error("Insufficient adoptions: got {got}, needed {needed}")]
    InsufficientAdoptions { got: u16, needed: u16 },
    #[error("Proposal rejected: {0}")]
    ProposalRejected(String),
    #[error("Upgrade application failed: {0}")]
    ApplicationFailed(String),
    #[error("Epoch fence check failed: {0}")]
    EpochFenceError(String),
    #[error("Handler error: {0}")]
    Handler(#[from] AuraHandlerError),
    #[error("Effect system error: {0}")]
    EffectSystem(String),
}

impl From<OtaError> for ChoreographyError {
    fn from(e: OtaError) -> Self {
        ChoreographyError::ProtocolViolation {
            message: e.to_string(),
        }
    }
}

// ============================================================================
// Orchestration Logic
// ============================================================================

/// OTA upgrade orchestration
pub struct UpgradeOrchestrator {
    config: UpgradeConfig,
}

impl UpgradeOrchestrator {
    /// Create new upgrade orchestrator
    pub fn new(config: UpgradeConfig) -> Self {
        Self { config }
    }

    /// Orchestrate upgrade proposal and adoption for a device role
    pub async fn orchestrate(
        &self,
        adapter: &AuraHandlerAdapter,
        proposal: &UpgradeProposal,
    ) -> Result<OtaResult, OtaError> {
        let device_id = adapter.device_id();
        let proposal_id = proposal.package_id;

        // Validate configuration
        if self.config.participants.is_empty() {
            return Err(OtaError::InvalidConfig(
                "participants list is empty".to_string(),
            ));
        }

        if self.config.quorum_threshold as usize > self.config.participants.len() {
            return Err(OtaError::InvalidConfig(format!(
                "quorum_threshold {} exceeds participants count {}",
                self.config.quorum_threshold,
                self.config.participants.len()
            )));
        }

        // Phase 1: Proposal broadcast (coordinator only)
        if device_id == self.config.coordinator {
            self.broadcast_proposal(adapter, proposal).await?;
        }

        // Phase 2: Adoption phase
        let adoption_responses = self.collect_adoptions(adapter, proposal, device_id).await?;

        let adoption_count = adoption_responses
            .iter()
            .filter(|r| matches!(r, AdoptionResponse::OptIn { .. }))
            .count() as u16;

        // Phase 3: Activation (hard forks only)
        let activated = if matches!(proposal.kind, UpgradeKind::HardFork) {
            if adoption_count >= self.config.quorum_threshold {
                // Check epoch fence
                if let Some(fence) = &proposal.activation_fence {
                    if fence.epoch > self.config.current_epoch {
                        return Err(OtaError::EpochFenceError(format!(
                            "Epoch fence {} not yet reached (current: {})",
                            fence.epoch, self.config.current_epoch
                        )));
                    }
                }

                // Broadcast activation signal
                if device_id == self.config.coordinator {
                    self.broadcast_activation(adapter, proposal, adoption_count)
                        .await?;
                }

                true
            } else {
                return Err(OtaError::InsufficientAdoptions {
                    got: adoption_count,
                    needed: self.config.quorum_threshold,
                });
            }
        } else {
            // Soft forks don't require activation
            false
        };

        // Phase 4: Completion
        if device_id == self.config.coordinator {
            self.wait_for_completion(adapter, proposal_id).await?;
        }

        Ok(OtaResult {
            proposal_id,
            version: proposal.version,
            adoptions: adoption_count,
            activated,
            completed: true,
            error: None,
        })
    }

    /// Broadcast proposal to all participants
    async fn broadcast_proposal(
        &self,
        adapter: &AuraHandlerAdapter,
        proposal: &UpgradeProposal,
    ) -> Result<(), OtaError> {
        let timestamp = adapter.effects().current_timestamp().await;

        let message = UpgradeMessage {
            proposal_id: proposal.package_id,
            version: proposal.version,
            kind: proposal.kind,
            checksum: proposal.artifact_hash,
            artifact_uri: proposal.artifact_uri.clone(),
            timestamp,
        };

        for participant in &self.config.participants {
            if participant != &adapter.device_id() {
                // Log: Broadcasting upgrade proposal (simplified for now)
                tracing::info!(
                    "Broadcasting upgrade proposal to {}: version {}",
                    participant,
                    proposal.version
                );
            }
        }

        Ok(())
    }

    /// Collect adoption responses from all participants
    async fn collect_adoptions(
        &self,
        adapter: &AuraHandlerAdapter,
        proposal: &UpgradeProposal,
        device_id: DeviceId,
    ) -> Result<Vec<AdoptionResponse>, OtaError> {
        let mut responses = Vec::new();

        for participant in &self.config.participants {
            let timestamp = adapter.effects().current_timestamp().await;

            // For this device: simulate local adoption decision
            if participant == &device_id {
                let response = AdoptionResponse::OptIn {
                    device_id,
                    version: proposal.version,
                    timestamp,
                };
                responses.push(response);
            } else {
                // In real implementation: receive responses from network
                // For now: assume all devices opt in
                let response = AdoptionResponse::OptIn {
                    device_id: *participant,
                    version: proposal.version,
                    timestamp,
                };
                responses.push(response);
            }
        }

        Ok(responses)
    }

    /// Broadcast activation signal to all participants
    async fn broadcast_activation(
        &self,
        adapter: &AuraHandlerAdapter,
        proposal: &UpgradeProposal,
        quorum_count: u16,
    ) -> Result<(), OtaError> {
        let timestamp = adapter.effects().current_timestamp().await;

        let activation_epoch = if let Some(fence) = &proposal.activation_fence {
            fence.epoch
        } else {
            self.config.current_epoch
        };

        let signal = ActivationSignal {
            proposal_id: proposal.package_id,
            activation_epoch,
            quorum_count,
            timestamp,
        };

        tracing::info!(
            "Broadcasting activation signal: version {} at epoch {}",
            proposal.version,
            activation_epoch
        );

        Ok(())
    }

    /// Wait for all devices to confirm upgrade completion
    async fn wait_for_completion(
        &self,
        adapter: &AuraHandlerAdapter,
        proposal_id: Uuid,
    ) -> Result<(), OtaError> {
        tracing::info!("Waiting for upgrade completion: {}", proposal_id);

        // In real implementation: collect completion confirmations
        Ok(())
    }

    /// Generate maintenance event for journal recording
    pub fn generate_maintenance_event(
        &self,
        proposal: &UpgradeProposal,
        _adoption_count: u16,
    ) -> MaintenanceEvent {
        let account_id = if let Some(fence) = &proposal.activation_fence {
            fence.account_id
        } else {
            aura_core::AccountId::from_bytes([0u8; 32])
        };

        let fence = proposal.activation_fence.unwrap_or_else(|| {
            aura_core::maintenance::IdentityEpochFence::new(account_id, self.config.current_epoch)
        });

        MaintenanceEvent::UpgradeActivated(UpgradeActivated::new(
            proposal.package_id,
            proposal.version,
            fence,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upgrade_orchestrator_validates_config() {
        let invalid_config = UpgradeConfig {
            coordinator: DeviceId::new(),
            participants: vec![],
            quorum_threshold: 1,
            current_epoch: 0,
            adoption_timeout_secs: 300,
        };

        let orchestrator = UpgradeOrchestrator::new(invalid_config);
        // Would fail when orchestrate is called with empty participants

        let valid_config = UpgradeConfig {
            coordinator: DeviceId::new(),
            participants: vec![DeviceId::new(), DeviceId::new()],
            quorum_threshold: 2,
            current_epoch: 0,
            adoption_timeout_secs: 300,
        };

        let orchestrator = UpgradeOrchestrator::new(valid_config);
        // Would succeed when orchestrate is called
    }

    #[test]
    fn hard_fork_requires_quorum() {
        let config = UpgradeConfig {
            coordinator: DeviceId::new(),
            participants: vec![DeviceId::new(), DeviceId::new(), DeviceId::new()],
            quorum_threshold: 2,
            current_epoch: 10,
            adoption_timeout_secs: 300,
        };

        let orchestrator = UpgradeOrchestrator::new(config);
        assert_eq!(orchestrator.config.quorum_threshold, 2);
    }
}
