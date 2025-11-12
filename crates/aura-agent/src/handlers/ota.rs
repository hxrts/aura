//! OTA Upgrade Handler
//!
//! This module provides device-side OTA upgrade handling that integrates
//! the choreography system with the agent runtime.

use crate::errors::Result as AgentResult;
use aura_core::{
    maintenance::{MaintenanceEvent, UpgradeKind, UpgradeProposal},
    DeviceId, Epoch,
};
use aura_protocol::choreography::protocols::ota::{OtaError, UpgradeConfig, UpgradeOrchestrator};
use aura_protocol::choreography::AuraHandlerAdapter;
use serde_json;
use std::collections::HashMap;
use tokio::sync::RwLock;
use tracing::{error, info, warn};
use uuid::Uuid;

/// OTA upgrade operations handler
pub struct OtaOperations {
    /// Device ID for this handler
    device_id: DeviceId,
    /// In-flight upgrades being coordinated
    upgrades: RwLock<HashMap<Uuid, UpgradeProposalState>>,
}

/// State tracking for an in-flight upgrade
#[derive(Debug, Clone)]
pub struct UpgradeProposalState {
    pub proposal_id: Uuid,
    pub proposal: UpgradeProposal,
    pub status: UpgradeStatus,
    pub adoptions: u32,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpgradeStatus {
    Proposed,
    OptedIn,
    Downloading,
    Downloaded,
    Applying,
    Completed,
    Failed,
    Rejected,
}

impl OtaOperations {
    /// Create new OTA operations handler
    pub fn new(device_id: DeviceId) -> Self {
        Self {
            device_id,
            upgrades: RwLock::new(HashMap::new()),
        }
    }

    /// Handle upgrade proposal from coordinator
    pub async fn handle_upgrade_proposal(
        &self,
        adapter: &AuraHandlerAdapter,
        proposal: UpgradeProposal,
        participants: Vec<DeviceId>,
        quorum_threshold: u16,
        current_epoch: Epoch,
    ) -> AgentResult<()> {
        info!(
            "Handling upgrade proposal: {} -> {}",
            proposal.version, proposal.package_id
        );

        let proposal_id = proposal.package_id;

        // Track the proposal
        {
            let mut upgrades = self.upgrades.write().await;
            upgrades.insert(
                proposal_id,
                UpgradeProposalState {
                    proposal_id,
                    proposal: proposal.clone(),
                    status: UpgradeStatus::Proposed,
                    adoptions: 0,
                    error: None,
                },
            );
        }

        // Create upgrade configuration
        let coordinator = participants.get(0).copied().unwrap_or(self.device_id);
        let config = UpgradeConfig {
            coordinator,
            participants,
            quorum_threshold,
            current_epoch,
            adoption_timeout_secs: 300,
        };

        // Create orchestrator
        let orchestrator = UpgradeOrchestrator::new(config);

        // Execute orchestration
        match orchestrator.orchestrate(adapter, &proposal).await {
            Ok(result) => {
                info!(
                    "Upgrade orchestration completed: {} adoptions, activated: {}",
                    result.adoptions, result.activated
                );

                // Update state
                let mut upgrades = self.upgrades.write().await;
                if let Some(state) = upgrades.get_mut(&proposal_id) {
                    state.status = if result.activated {
                        UpgradeStatus::Completed
                    } else {
                        UpgradeStatus::OptedIn
                    };
                    state.adoptions = result.adoptions as u32;
                }

                // Emit maintenance event if hard fork activated
                if result.activated {
                    let event =
                        orchestrator.generate_maintenance_event(&proposal, result.adoptions);
                    info!(
                        "Emitting maintenance event for activated upgrade: {:?}",
                        event
                    );

                    // Persist event to journal as a fact for CRDT replication
                    match Self::emit_maintenance_event(adapter, event).await {
                        Ok(()) => info!("Maintenance event persisted to journal"),
                        Err(e) => warn!("Failed to persist maintenance event: {}", e),
                    }
                }

                Ok(())
            }
            Err(e) => {
                error!("Upgrade orchestration failed: {}", e);

                // Update state with error
                let mut upgrades = self.upgrades.write().await;
                if let Some(state) = upgrades.get_mut(&proposal_id) {
                    state.status = UpgradeStatus::Failed;
                    state.error = Some(e.to_string());
                }

                Err(aura_core::AuraError::coordination_failed(e.to_string()).into())
            }
        }
    }

    /// Get current status of an upgrade
    pub async fn get_upgrade_status(&self, proposal_id: Uuid) -> Option<UpgradeProposalState> {
        let upgrades = self.upgrades.read().await;
        upgrades.get(&proposal_id).cloned()
    }

    /// Get all in-flight upgrades
    pub async fn list_upgrades(&self) -> Vec<UpgradeProposalState> {
        let upgrades = self.upgrades.read().await;
        upgrades.values().cloned().collect()
    }

    /// Opt into an upgrade
    pub async fn opt_in(&self, proposal_id: Uuid) -> AgentResult<()> {
        info!("Opting into upgrade: {}", proposal_id);

        let mut upgrades = self.upgrades.write().await;
        if let Some(state) = upgrades.get_mut(&proposal_id) {
            if state.status == UpgradeStatus::Proposed {
                state.status = UpgradeStatus::OptedIn;
                state.adoptions += 1;
                Ok(())
            } else {
                Err(aura_core::AuraError::internal(&format!(
                    "Cannot opt in to upgrade in status: {:?}",
                    state.status
                ))
                .into())
            }
        } else {
            Err(aura_core::AuraError::not_found(format!(
                "Upgrade proposal not found: {}",
                proposal_id
            ))
            .into())
        }
    }

    /// Reject an upgrade
    pub async fn reject(&self, proposal_id: Uuid, reason: String) -> AgentResult<()> {
        info!("Rejecting upgrade {}: {}", proposal_id, reason);

        let mut upgrades = self.upgrades.write().await;
        if let Some(state) = upgrades.get_mut(&proposal_id) {
            state.status = UpgradeStatus::Rejected;
            state.error = Some(reason);
            Ok(())
        } else {
            Err(aura_core::AuraError::not_found(format!(
                "Upgrade proposal not found: {}",
                proposal_id
            ))
            .into())
        }
    }

    /// Mark upgrade as completed locally
    pub async fn mark_upgrade_completed(&self, proposal_id: Uuid) -> AgentResult<()> {
        info!("Marking upgrade as completed: {}", proposal_id);

        let mut upgrades = self.upgrades.write().await;
        if let Some(state) = upgrades.get_mut(&proposal_id) {
            state.status = UpgradeStatus::Completed;
            Ok(())
        } else {
            Err(aura_core::AuraError::not_found(format!(
                "Upgrade proposal not found: {}",
                proposal_id
            ))
            .into())
        }
    }

    /// Emit maintenance event to journal for replication
    async fn emit_maintenance_event(
        adapter: &AuraHandlerAdapter,
        event: aura_core::maintenance::MaintenanceEvent,
    ) -> AgentResult<()> {
        use aura_core::effects::JournalEffects;

        // Serialize event to journal fact
        let event_json = serde_json::to_string(&event)
            .map_err(|e| aura_core::AuraError::internal(&format!("Serialization failed: {}", e)))?;

        // Create journal delta with maintenance event fact
        let mut delta_journal = aura_core::Journal::default();
        delta_journal.facts.insert(
            format!("maintenance_event_{}", uuid::Uuid::new_v4()),
            aura_core::FactValue::String(event_json),
        );

        // Merge into current journal via JournalEffects trait methods
        let effects = adapter.effects();
        let current = effects.get_journal().await?;
        let updated = effects.merge_facts(&current, &delta_journal).await?;

        // Persist updated journal
        effects.persist_journal(&updated).await?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn ota_operations_tracks_proposals() {
        let device_id = DeviceId(uuid::Uuid::new_v4());
        let ops = OtaOperations::new(device_id);

        // Should start with no upgrades
        let upgrades = ops.list_upgrades().await;
        assert!(upgrades.is_empty());
    }

    #[tokio::test]
    async fn ota_operations_opt_in() {
        let device_id = DeviceId(uuid::Uuid::new_v4());
        let ops = OtaOperations::new(device_id);
        let proposal_id = Uuid::new_v4();

        // Create a test proposal state
        {
            let mut upgrades = ops.upgrades.write().await;
            upgrades.insert(
                proposal_id,
                UpgradeProposalState {
                    proposal_id,
                    proposal: UpgradeProposal {
                        package_id: proposal_id,
                        version: aura_core::SemanticVersion::new(1, 0, 0),
                        artifact_hash: aura_core::Hash32([0u8; 32]),
                        artifact_uri: None,
                        kind: UpgradeKind::SoftFork,
                        activation_fence: None,
                    },
                    status: UpgradeStatus::Proposed,
                    adoptions: 0,
                    error: None,
                },
            );
        }

        // Opt in should succeed
        assert!(ops.opt_in(proposal_id).await.is_ok());

        // Check status
        let state = ops.get_upgrade_status(proposal_id).await;
        assert!(state.is_some());
        assert_eq!(state.unwrap().status, UpgradeStatus::OptedIn);
    }
}
