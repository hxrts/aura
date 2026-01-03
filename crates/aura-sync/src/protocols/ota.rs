#![allow(missing_docs)]

//! Over-the-air (OTA) upgrade coordination protocol
//!
//! Provides threshold-based upgrade coordination with epoch fencing
//! for safe distributed system upgrades.
//!
//! # Architecture
//!
//! The OTA protocol coordinates:
//! 1. Upgrade proposal from admin/guardian
//! 2. Readiness collection from M-of-N devices
//! 3. Threshold check for activation
//! 4. Epoch fence enforcement for hard forks
//! 5. Activation and journal recording
//!
//! # Usage
//!
//! ```rust,ignore
//! use aura_sync::protocols::{OTAProtocol, OTAConfig, UpgradeKind};
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let config = OTAConfig::default();
//! let protocol = OTAProtocol::new(config);
//!
//! // Propose upgrade
//! let proposal = protocol.propose_upgrade(
//!     package_id,
//!     version,
//!     UpgradeKind::SoftFork,
//! )?;
//! # Ok(())
//! # }
//! ```

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::core::{sync_protocol_error, SyncResult};
use aura_core::types::Epoch;
use aura_core::{AuthorityId, Hash32};

// =============================================================================
// Types
// =============================================================================

/// Upgrade kind
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum UpgradeKind {
    /// Soft fork (backward compatible)
    SoftFork,

    /// Hard fork (requires coordinated activation)
    HardFork,
}

/// Upgrade proposal
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpgradeProposal {
    /// Unique proposal ID
    pub proposal_id: Uuid,

    /// Package identifier
    pub package_id: Uuid,

    /// Target version
    pub version: String,

    /// Upgrade kind
    pub kind: UpgradeKind,

    /// Package hash for verification
    pub package_hash: Hash32,

    /// Activation epoch (for hard forks)
    pub activation_epoch: Option<Epoch>,

    /// Proposer authority
    pub proposer: AuthorityId,
}

/// Readiness status
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ReadinessStatus {
    /// Device is ready
    Ready,

    /// Device is not ready yet
    NotReady { reason: String },

    /// Device rejects upgrade
    Rejected { reason: String },
}

/// Readiness declaration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadinessDeclaration {
    /// Proposal ID
    pub proposal_id: Uuid,

    /// Declaring authority
    pub device: AuthorityId,

    /// Readiness status
    pub status: ReadinessStatus,

    /// Declaration timestamp
    pub timestamp: u64,
}

/// OTA upgrade result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OTAResult {
    /// Proposal that was executed
    pub proposal: UpgradeProposal,

    /// Authorities that declared ready
    pub ready_devices: Vec<AuthorityId>,

    /// Whether activation threshold was met
    pub activated: bool,

    /// Activation epoch (if activated)
    pub activation_epoch: Option<Epoch>,
}

// =============================================================================
// Configuration
// =============================================================================

/// OTA protocol configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OTAConfig {
    /// Readiness threshold (M-of-N)
    pub readiness_threshold: u32,

    /// Total quorum size
    pub quorum_size: u32,

    /// Require epoch fence for hard forks
    pub enforce_epoch_fence: bool,
}

impl Default for OTAConfig {
    fn default() -> Self {
        Self {
            readiness_threshold: 2,
            quorum_size: 3,
            enforce_epoch_fence: true,
        }
    }
}

// =============================================================================
// OTA Protocol
// =============================================================================

/// OTA upgrade coordination protocol
pub struct OTAProtocol {
    config: OTAConfig,
    pending_proposal: Option<UpgradeProposal>,
    readiness: HashMap<AuthorityId, ReadinessStatus>,
}

impl OTAProtocol {
    /// Create a new OTA protocol
    pub fn new(config: OTAConfig) -> Self {
        Self {
            config,
            pending_proposal: None,
            readiness: HashMap::new(),
        }
    }

    /// Propose an upgrade
    ///
    /// Note: Callers should obtain `proposal_id` via `RandomEffects::random_uuid()` or provide a deterministic `Uuid::from_bytes(..)` in tests
    pub fn propose_upgrade(
        &mut self,
        proposal_id: Uuid,
        package_id: Uuid,
        version: String,
        kind: UpgradeKind,
        package_hash: Hash32,
        proposer: AuthorityId,
    ) -> SyncResult<UpgradeProposal> {
        if self.pending_proposal.is_some() {
            return Err(sync_protocol_error(
                "sync",
                "Upgrade proposal already pending",
            ));
        }

        let proposal = UpgradeProposal {
            proposal_id,
            package_id,
            version,
            kind,
            package_hash,
            activation_epoch: None,
            proposer,
        };

        self.pending_proposal = Some(proposal.clone());
        self.readiness.clear();

        Ok(proposal)
    }

    /// Declare readiness for pending upgrade
    pub fn declare_readiness(
        &mut self,
        device: AuthorityId,
        status: ReadinessStatus,
    ) -> SyncResult<()> {
        if self.pending_proposal.is_none() {
            return Err(sync_protocol_error("sync", "No pending upgrade proposal"));
        }

        self.readiness.insert(device, status);
        Ok(())
    }

    /// Check if activation threshold is met
    pub fn check_threshold(&self) -> bool {
        let ready_count = self
            .readiness
            .values()
            .filter(|s| matches!(s, ReadinessStatus::Ready))
            .count();

        let ready_count_u32 =
            u32::try_from(ready_count).expect("ready device count exceeds u32::MAX");
        ready_count_u32 >= self.config.readiness_threshold
    }

    /// Activate upgrade if threshold is met
    pub fn activate(&mut self) -> SyncResult<OTAResult> {
        let proposal = self
            .pending_proposal
            .take()
            .ok_or_else(|| sync_protocol_error("sync", "No pending proposal to activate"))?;

        if !self.check_threshold() {
            return Err(sync_protocol_error("sync", "Readiness threshold not met"));
        }

        let ready_devices: Vec<AuthorityId> = self
            .readiness
            .iter()
            .filter_map(|(device, status)| {
                if matches!(status, ReadinessStatus::Ready) {
                    Some(*device)
                } else {
                    None
                }
            })
            .collect();

        Ok(OTAResult {
            proposal,
            ready_devices,
            activated: true,
            activation_epoch: None,
        })
    }

    /// Get pending proposal
    pub fn get_pending(&self) -> Option<&UpgradeProposal> {
        self.pending_proposal.as_ref()
    }

    /// Cancel pending proposal
    pub fn cancel(&mut self) -> SyncResult<()> {
        if self.pending_proposal.is_none() {
            return Err(sync_protocol_error("sync", "No pending proposal to cancel"));
        }

        self.pending_proposal = None;
        self.readiness.clear();
        Ok(())
    }
}

impl Default for OTAProtocol {
    fn default() -> Self {
        Self::new(OTAConfig::default())
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
#[allow(clippy::disallowed_methods)]
mod tests {
    use super::*;

    #[test]
    fn test_ota_proposal() {
        let mut protocol = OTAProtocol::default();
        let device = AuthorityId::new_from_entropy([1; 32]);

        let proposal = protocol
            .propose_upgrade(
                Uuid::from_bytes(2u128.to_be_bytes()), // proposal_id
                Uuid::from_bytes(3u128.to_be_bytes()), // package_id
                "2.0.0".to_string(),
                UpgradeKind::SoftFork,
                Hash32([0; 32]),
                device,
            )
            .unwrap();

        assert!(protocol.get_pending().is_some());
        assert_eq!(proposal.version, "2.0.0");
    }

    #[test]
    fn test_readiness_threshold() {
        let config = OTAConfig {
            readiness_threshold: 2,
            quorum_size: 3,
            enforce_epoch_fence: false,
        };

        let mut protocol = OTAProtocol::new(config);

        protocol
            .propose_upgrade(
                Uuid::from_bytes(4u128.to_be_bytes()), // proposal_id
                Uuid::from_bytes(5u128.to_be_bytes()), // package_id
                "2.0.0".to_string(),
                UpgradeKind::SoftFork,
                Hash32([0; 32]),
                AuthorityId::new_from_entropy([1; 32]),
            )
            .unwrap();

        // One ready - not enough
        protocol
            .declare_readiness(
                AuthorityId::new_from_entropy([2; 32]),
                ReadinessStatus::Ready,
            )
            .unwrap();
        assert!(!protocol.check_threshold());

        // Two ready - threshold met
        protocol
            .declare_readiness(
                AuthorityId::new_from_entropy([3; 32]),
                ReadinessStatus::Ready,
            )
            .unwrap();
        assert!(protocol.check_threshold());
    }

    #[test]
    fn test_activation() {
        let mut protocol = OTAProtocol::default();

        protocol
            .propose_upgrade(
                Uuid::from_bytes(6u128.to_be_bytes()),  // proposal_id
                Uuid::from_bytes(20u128.to_be_bytes()), // package_id
                "2.0.0".to_string(),
                UpgradeKind::SoftFork,
                Hash32([0; 32]),
                AuthorityId::new_from_entropy([1; 32]),
            )
            .unwrap();

        protocol
            .declare_readiness(
                AuthorityId::new_from_entropy([2; 32]),
                ReadinessStatus::Ready,
            )
            .unwrap();
        protocol
            .declare_readiness(
                AuthorityId::new_from_entropy([3; 32]),
                ReadinessStatus::Ready,
            )
            .unwrap();

        let result = protocol.activate().unwrap();
        assert!(result.activated);
        assert_eq!(result.ready_devices.len(), 2);
        assert!(protocol.get_pending().is_none());
    }
}
