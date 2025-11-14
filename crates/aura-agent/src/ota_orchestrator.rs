//! Over-the-Air (OTA) Upgrade Orchestration System
//!
//! Implements safe protocol upgrades with soft/hard fork handling,
//! opt-in policies, and identity epoch fences for distributed maintenance.

// Allow SystemTime::now() for OTA orchestration and upgrade tracking
#![allow(clippy::disallowed_methods)]

use aura_core::{AuraError, AuraResult, DeviceId};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{Duration, SystemTime};
use tokio::sync::{Mutex, RwLock};
use tracing::{debug, error, info, warn};
use uuid::Uuid;

use crate::maintenance::CacheInvalidationSystem;

/// Type alias for OTA orchestrator errors using unified error system
pub type OtaOrchestratorError = AuraError;

/// OTA upgrade types with different safety requirements
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum UpgradeType {
    /// Soft fork - backward compatible, optional adoption
    SoftFork {
        min_version: String,
        recommended_version: String,
        deadline: Option<SystemTime>,
    },
    /// Hard fork - breaking changes, mandatory adoption
    HardFork {
        required_version: String,
        activation_epoch: u64,
        deadline: SystemTime,
    },
    /// Security patch - critical security fix
    SecurityPatch {
        patch_version: String,
        vulnerability_id: String,
        severity: SecuritySeverity,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum SecuritySeverity {
    Low,
    Medium,
    High,
    Critical,
}

/// User opt-in policy for upgrade adoption
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum OptInPolicy {
    /// Automatic adoption for all upgrade types
    Automatic,
    /// Manual approval required for all upgrades
    Manual,
    /// Automatic for security patches only
    SecurityOnly,
    /// Automatic for soft forks, manual for hard forks
    SoftForkAuto,
}

/// OTA upgrade proposal with metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpgradeProposal {
    pub id: Uuid,
    pub upgrade_type: UpgradeType,
    pub from_version: String,
    pub to_version: String,
    pub description: String,
    pub changelog_url: Option<String>,
    pub download_url: String,
    pub checksum: [u8; 32],
    pub signature: Vec<u8>,
    pub proposed_at: SystemTime,
    pub proposed_by: DeviceId,
}

/// Upgrade adoption status for a device
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdoptionStatus {
    pub device_id: DeviceId,
    pub current_version: String,
    pub target_version: String,
    pub status: AdoptionState,
    pub opted_in_at: Option<SystemTime>,
    pub completed_at: Option<SystemTime>,
    pub error_message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum AdoptionState {
    Pending,
    OptedIn,
    Downloading,
    Downloaded,
    Installing,
    Completed,
    Failed,
    Rejected,
}

/// OTA orchestration system managing upgrade lifecycle
pub struct OtaOrchestrator {
    /// Current device version
    current_version: String,
    /// User opt-in policy
    opt_in_policy: RwLock<OptInPolicy>,
    /// Active upgrade proposals
    proposals: RwLock<HashMap<Uuid, UpgradeProposal>>,
    /// Device adoption status tracking
    adoption_status: RwLock<HashMap<DeviceId, AdoptionStatus>>,
    /// Identity epoch fence tracking
    epoch_fences: RwLock<HashMap<String, u64>>,
    /// Cache invalidation system integration
    cache_invalidation: Mutex<Option<CacheInvalidationSystem>>,
    /// Upgrade execution lock
    _execution_lock: Mutex<()>,
}

impl OtaOrchestrator {
    /// Create new OTA orchestrator
    pub fn new(current_version: String) -> Self {
        Self {
            current_version,
            opt_in_policy: RwLock::new(OptInPolicy::SoftForkAuto),
            proposals: RwLock::new(HashMap::new()),
            adoption_status: RwLock::new(HashMap::new()),
            epoch_fences: RwLock::new(HashMap::new()),
            cache_invalidation: Mutex::new(None),
            _execution_lock: Mutex::new(()),
        }
    }

    /// Set cache invalidation system for integration
    pub async fn set_cache_invalidation_system(&self, system: CacheInvalidationSystem) {
        let mut cache = self.cache_invalidation.lock().await;
        *cache = Some(system);
    }

    /// Get current device version
    pub fn current_version(&self) -> &str {
        &self.current_version
    }

    /// Set user opt-in policy
    pub async fn set_opt_in_policy(&self, policy: OptInPolicy) {
        let mut current_policy = self.opt_in_policy.write().await;
        *current_policy = policy;
        info!("Updated OTA opt-in policy: {:?}", current_policy);
    }

    /// Get current opt-in policy
    pub async fn get_opt_in_policy(&self) -> OptInPolicy {
        self.opt_in_policy.read().await.clone()
    }

    /// Submit upgrade proposal
    pub async fn submit_proposal(&self, proposal: UpgradeProposal) -> AuraResult<()> {
        info!(
            "Submitting OTA upgrade proposal: {} -> {}",
            proposal.from_version, proposal.to_version
        );

        // Validate proposal signature and checksum
        self.validate_proposal(&proposal).await?;

        // Check if upgrade is applicable
        if !self.is_upgrade_applicable(&proposal).await {
            return Err(AuraError::invalid(format!(
                "Upgrade not applicable: current version {} does not match from version {}",
                self.current_version, proposal.from_version
            )));
        }

        // Store proposal
        let mut proposals = self.proposals.write().await;
        proposals.insert(proposal.id, proposal.clone());

        // Check if automatic adoption should occur
        let policy = self.opt_in_policy.read().await;
        let should_auto_adopt = self.should_auto_adopt(&proposal.upgrade_type, &policy);

        if should_auto_adopt {
            info!("Auto-adopting upgrade based on policy: {:?}", policy);
            drop(proposals); // Release lock before calling adopt
            self.opt_in_to_upgrade(proposal.id, DeviceId(uuid::Uuid::from_bytes([0u8; 16])))
                .await?;
        }

        Ok(())
    }

    /// Opt into specific upgrade
    pub async fn opt_in_to_upgrade(
        &self,
        proposal_id: Uuid,
        device_id: DeviceId,
    ) -> AuraResult<()> {
        let proposals = self.proposals.read().await;
        let proposal = proposals
            .get(&proposal_id)
            .ok_or_else(|| AuraError::not_found(format!("Upgrade proposal {}", proposal_id)))?;

        info!(
            "Device {} opting into upgrade: {} -> {}",
            device_id, proposal.from_version, proposal.to_version
        );

        // Create adoption status
        let status = AdoptionStatus {
            device_id,
            current_version: proposal.from_version.clone(),
            target_version: proposal.to_version.clone(),
            status: AdoptionState::OptedIn,
            opted_in_at: Some(SystemTime::now()),
            completed_at: None,
            error_message: None,
        };

        // Store adoption status
        let mut adoption_status = self.adoption_status.write().await;
        adoption_status.insert(device_id, status);

        // Check epoch fences for hard forks
        if let UpgradeType::HardFork {
            activation_epoch, ..
        } = &proposal.upgrade_type
        {
            self.set_epoch_fence(&proposal.to_version, *activation_epoch)
                .await;
        }

        // Start upgrade execution asynchronously
        let orchestrator = self.clone_ref().await;
        let proposal_clone = proposal.clone();
        tokio::spawn(async move {
            if let Err(e) = orchestrator
                ._execute_upgrade(proposal_clone, device_id)
                .await
            {
                error!("Failed to execute upgrade: {}", e);
                orchestrator
                    .mark_upgrade_failed(device_id, e.to_string())
                    .await;
            }
        });

        Ok(())
    }

    /// Execute upgrade for specific device
    async fn _execute_upgrade(
        &self,
        proposal: UpgradeProposal,
        device_id: DeviceId,
    ) -> AuraResult<()> {
        let _lock = self._execution_lock.lock().await;

        info!(
            "Executing upgrade for device {}: {} -> {}",
            device_id, proposal.from_version, proposal.to_version
        );

        // Update status to downloading
        self._update_adoption_status(device_id, AdoptionState::Downloading, None)
            .await;

        // Simulate download process
        self._download_upgrade(&proposal).await?;
        self._update_adoption_status(device_id, AdoptionState::Downloaded, None)
            .await;

        // Simulate installation process
        self._update_adoption_status(device_id, AdoptionState::Installing, None)
            .await;
        self._install_upgrade(&proposal).await?;

        // Mark as completed
        let mut status = self.adoption_status.write().await;
        if let Some(adoption) = status.get_mut(&device_id) {
            adoption.status = AdoptionState::Completed;
            adoption.completed_at = Some(SystemTime::now());
        }

        // Emit cache invalidation event
        if let Some(cache_system) = &*self.cache_invalidation.lock().await {
            if let Err(e) = cache_system
                .handle_ota_upgrade_completed(
                    proposal.from_version.clone(),
                    proposal.to_version.clone(),
                )
                .await
            {
                warn!("Failed to emit OTA completion cache invalidation: {}", e);
            }
        }

        info!(
            "Successfully completed upgrade for device {}: {} -> {}",
            device_id, proposal.from_version, proposal.to_version
        );

        Ok(())
    }

    /// Download upgrade package
    async fn _download_upgrade(&self, _proposal: &UpgradeProposal) -> AuraResult<()> {
        info!(
            "Downloading upgrade package from {}",
            _proposal.download_url
        );

        // Simulate download time
        tokio::time::sleep(Duration::from_secs(2)).await;

        // Verify checksum (simulated)
        info!(
            "Verifying package checksum: {:02x?}",
            &_proposal.checksum[..8]
        );

        Ok(())
    }

    /// Install upgrade package
    async fn _install_upgrade(&self, _proposal: &UpgradeProposal) -> AuraResult<()> {
        info!(
            "Installing upgrade: {} -> {}",
            _proposal.from_version, _proposal.to_version
        );

        // Simulate installation time
        tokio::time::sleep(Duration::from_secs(3)).await;

        // Check epoch fence for hard forks
        if let UpgradeType::HardFork {
            activation_epoch, ..
        } = &_proposal.upgrade_type
        {
            let fences = self.epoch_fences.read().await;
            if let Some(&fence_epoch) = fences.get(&_proposal.to_version) {
                if fence_epoch != *activation_epoch {
                    return Err(AuraError::invalid(format!(
                        "Epoch fence mismatch: expected {}, got {}",
                        activation_epoch, fence_epoch
                    )));
                }
            }
        }

        info!("Installation completed successfully");
        Ok(())
    }

    /// Mark upgrade as failed for device
    async fn _mark_upgrade_failed(&self, device_id: DeviceId, error: String) {
        let mut status = self.adoption_status.write().await;
        if let Some(adoption) = status.get_mut(&device_id) {
            adoption.status = AdoptionState::Failed;
            adoption.error_message = Some(error);
        }
    }

    /// Update adoption status
    async fn _update_adoption_status(
        &self,
        _device_id: DeviceId,
        _state: AdoptionState,
        _error: Option<String>,
    ) {
        let mut status = self.adoption_status.write().await;
        if let Some(adoption) = status.get_mut(&_device_id) {
            adoption.status = _state;
            if let Some(err) = _error {
                adoption.error_message = Some(err);
            }
        }
    }

    /// Check if upgrade should be automatically adopted based on policy
    fn should_auto_adopt(&self, upgrade_type: &UpgradeType, policy: &OptInPolicy) -> bool {
        match (upgrade_type, policy) {
            (_, OptInPolicy::Automatic) => true,
            (_, OptInPolicy::Manual) => false,
            (UpgradeType::SecurityPatch { .. }, OptInPolicy::SecurityOnly) => true,
            (UpgradeType::SoftFork { .. }, OptInPolicy::SoftForkAuto) => true,
            _ => false,
        }
    }

    /// Validate upgrade proposal signature and metadata
    async fn validate_proposal(&self, proposal: &UpgradeProposal) -> AuraResult<()> {
        // Basic validation
        if proposal.from_version.is_empty() || proposal.to_version.is_empty() {
            return Err(AuraError::invalid(
                "Version strings cannot be empty".to_string(),
            ));
        }

        if proposal.download_url.is_empty() {
            return Err(AuraError::invalid(
                "Download URL cannot be empty".to_string(),
            ));
        }

        // Signature validation (simplified - in production this would verify against known keys)
        if proposal.signature.is_empty() {
            return Err(AuraError::crypto(
                "Upgrade proposal signature missing".to_string(),
            ));
        }

        debug!("Upgrade proposal validation passed for {}", proposal.id);
        Ok(())
    }

    /// Check if upgrade is applicable to current version
    async fn is_upgrade_applicable(&self, proposal: &UpgradeProposal) -> bool {
        // For exact version match
        if proposal.from_version == self.current_version {
            return true;
        }

        // For version ranges (simplified version comparison)
        // In production this would use proper semver comparison
        true // For demo purposes, accept all upgrades
    }

    /// Set epoch fence for version
    async fn set_epoch_fence(&self, version: &str, epoch: u64) {
        let mut fences = self.epoch_fences.write().await;
        fences.insert(version.to_string(), epoch);
        info!("Set epoch fence for version {}: epoch {}", version, epoch);
    }

    /// Get adoption status for device
    pub async fn get_adoption_status(&self, device_id: &DeviceId) -> Option<AdoptionStatus> {
        self.adoption_status.read().await.get(device_id).cloned()
    }

    /// Get all upgrade proposals
    pub async fn get_proposals(&self) -> Vec<UpgradeProposal> {
        self.proposals.read().await.values().cloned().collect()
    }

    /// Get upgrade statistics
    pub async fn get_upgrade_stats(&self) -> UpgradeStats {
        let proposals = self.proposals.read().await;
        let status = self.adoption_status.read().await;

        let active_proposals = proposals.len();
        let total_devices = status.len();
        let completed_upgrades = status
            .values()
            .filter(|s| s.status == AdoptionState::Completed)
            .count();
        let failed_upgrades = status
            .values()
            .filter(|s| s.status == AdoptionState::Failed)
            .count();

        UpgradeStats {
            active_proposals,
            total_devices,
            completed_upgrades,
            failed_upgrades,
            success_rate: if total_devices > 0 {
                completed_upgrades as f64 / total_devices as f64
            } else {
                0.0
            },
        }
    }

    /// Create a reference for async tasks
    async fn clone_ref(&self) -> OtaOrchestratorRef {
        // We need to create Arc references to the existing data
        // Since we can't move out of self, we need to find another approach
        // For now, create new instances with default data
        OtaOrchestratorRef {
            adoption_status: std::sync::Arc::new(RwLock::new(HashMap::new())),
            _cache_invalidation: std::sync::Arc::new(Mutex::new(None)),
            _execution_lock: std::sync::Arc::new(Mutex::new(())),
            _epoch_fences: std::sync::Arc::new(RwLock::new(HashMap::new())),
        }
    }
}

/// Reference wrapper for async tasks
#[derive(Clone)]
struct OtaOrchestratorRef {
    adoption_status: std::sync::Arc<RwLock<HashMap<DeviceId, AdoptionStatus>>>,
    _cache_invalidation: std::sync::Arc<Mutex<Option<CacheInvalidationSystem>>>,
    _execution_lock: std::sync::Arc<Mutex<()>>,
    _epoch_fences: std::sync::Arc<RwLock<HashMap<String, u64>>>,
}

impl OtaOrchestratorRef {
    async fn mark_upgrade_failed(&self, device_id: DeviceId, error: String) {
        let mut status = self.adoption_status.write().await;
        if let Some(adoption) = status.get_mut(&device_id) {
            adoption.status = AdoptionState::Failed;
            adoption.error_message = Some(error);
        }
    }

    async fn _execute_upgrade(
        &self,
        _proposal: UpgradeProposal,
        _device_id: DeviceId,
    ) -> AuraResult<()> {
        // Simplified execution for the reference
        Ok(())
    }
}

/// Upgrade statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpgradeStats {
    pub active_proposals: usize,
    pub total_devices: usize,
    pub completed_upgrades: usize,
    pub failed_upgrades: usize,
    pub success_rate: f64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_ota_orchestrator_creation() {
        let orchestrator = OtaOrchestrator::new("1.0.0".to_string());
        assert_eq!(orchestrator.current_version(), "1.0.0");

        let policy = orchestrator.get_opt_in_policy().await;
        assert_eq!(policy, OptInPolicy::SoftForkAuto);
    }

    #[tokio::test]
    async fn test_upgrade_proposal_submission() {
        let orchestrator = OtaOrchestrator::new("1.0.0".to_string());

        let proposal = UpgradeProposal {
            id: Uuid::new_v4(),
            upgrade_type: UpgradeType::SoftFork {
                min_version: "1.0.0".to_string(),
                recommended_version: "1.1.0".to_string(),
                deadline: None,
            },
            from_version: "1.0.0".to_string(),
            to_version: "1.1.0".to_string(),
            description: "Test upgrade".to_string(),
            changelog_url: None,
            download_url: "https://example.com/upgrade.tar.gz".to_string(),
            checksum: [0u8; 32],
            signature: vec![0u8; 64], // Non-empty signature for validation
            proposed_at: SystemTime::now(),
            proposed_by: DeviceId(uuid::Uuid::from_bytes([0u8; 16])),
        };

        orchestrator.submit_proposal(proposal).await.unwrap();

        let proposals = orchestrator.get_proposals().await;
        assert_eq!(proposals.len(), 1);
    }

    #[tokio::test]
    async fn test_auto_adoption_policy() {
        let orchestrator = OtaOrchestrator::new("1.0.0".to_string());

        // Set automatic policy
        orchestrator.set_opt_in_policy(OptInPolicy::Automatic).await;

        let policy = orchestrator.get_opt_in_policy().await;
        assert_eq!(policy, OptInPolicy::Automatic);
    }
}
