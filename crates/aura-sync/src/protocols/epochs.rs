//! Epoch Management Pattern
//!
//! Generic epoch rotation coordination pattern.

use crate::core::{sync_protocol_error, SyncError};
use aura_core::{ContextId, DeviceId};
use aura_effects::time::wallclock_ms;
// Note: aura-sync intentionally avoids aura-macros for semantic independence
// use aura_macros::choreography;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::SystemTime;

fn now_ms() -> u64 {
    wallclock_ms()
}

/// Epoch rotation coordinator using choreographic protocols
#[derive(Debug, Clone)]
pub struct EpochRotationCoordinator {
    #[allow(dead_code)]
    device_id: DeviceId,
    current_epoch: u64,
    epoch_config: EpochConfig,
    pending_rotations: HashMap<String, EpochRotation>,
}

/// Configuration for epoch management
#[derive(Debug, Clone)]
pub struct EpochConfig {
    pub epoch_duration: std::time::Duration,
    pub rotation_threshold: usize, // Minimum participants for rotation
    pub synchronization_timeout: std::time::Duration,
}

/// Epoch rotation state
#[derive(Debug, Clone)]
pub struct EpochRotation {
    pub rotation_id: String,
    pub target_epoch: u64,
    pub participants: Vec<DeviceId>,
    pub confirmations: HashMap<DeviceId, EpochConfirmation>,
    pub initiated_at_ms: u64,
    pub status: RotationStatus,
}

/// Rotation status enumeration
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RotationStatus {
    Initiated,
    Gathering,
    Synchronizing,
    Completed,
    Failed(String),
}

/// Epoch rotation proposal
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EpochRotationProposal {
    pub rotation_id: String,
    pub proposed_epoch: u64,
    pub proposer_id: DeviceId,
    pub participants: Vec<DeviceId>,
    pub context_id: ContextId,
    pub timestamp_ms: u64,
}

/// Epoch confirmation from participant
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EpochConfirmation {
    pub rotation_id: String,
    pub participant_id: DeviceId,
    pub current_epoch: u64,
    pub ready_for_epoch: u64,
    pub confirmation_timestamp_ms: u64,
}

/// Synchronized epoch commit
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EpochCommit {
    pub rotation_id: String,
    pub committed_epoch: u64,
    pub commit_timestamp_ms: u64,
    pub participants: Vec<DeviceId>,
}

impl Default for EpochConfig {
    fn default() -> Self {
        Self {
            epoch_duration: std::time::Duration::from_secs(300), // 5 minutes
            rotation_threshold: 2,
            synchronization_timeout: std::time::Duration::from_secs(30),
        }
    }
}

impl EpochRotationCoordinator {
    /// Create new epoch rotation coordinator
    pub fn new(device_id: DeviceId, initial_epoch: u64, config: EpochConfig) -> Self {
        Self {
            device_id,
            current_epoch: initial_epoch,
            epoch_config: config,
            pending_rotations: HashMap::new(),
        }
    }

    /// Get current epoch
    pub fn current_epoch(&self) -> u64 {
        self.current_epoch
    }

    /// Check if epoch rotation is needed
    pub fn needs_rotation(&self, last_rotation: SystemTime) -> bool {
        last_rotation.elapsed().unwrap_or_default() >= self.epoch_config.epoch_duration
    }

    /// Initiate epoch rotation
    pub fn initiate_rotation(
        &mut self,
        participants: Vec<DeviceId>,
        _context_id: ContextId,
    ) -> Result<String, SyncError> {
        if participants.len() < self.epoch_config.rotation_threshold {
            return Err(sync_protocol_error(
                "epochs",
                format!(
                    "Insufficient participants: {} < {}",
                    participants.len(),
                    self.epoch_config.rotation_threshold
                ),
            ));
        }

        let rotation_id = format!("epoch-rotation-{}-{}", self.current_epoch + 1, now_ms());

        let rotation = EpochRotation {
            rotation_id: rotation_id.clone(),
            target_epoch: self.current_epoch + 1,
            participants: participants.clone(),
            confirmations: HashMap::new(),
            initiated_at_ms: now_ms(),
            status: RotationStatus::Initiated,
        };

        self.pending_rotations.insert(rotation_id.clone(), rotation);
        Ok(rotation_id)
    }

    /// Process epoch confirmation
    pub fn process_confirmation(
        &mut self,
        confirmation: EpochConfirmation,
    ) -> Result<bool, SyncError> {
        let rotation = self
            .pending_rotations
            .get_mut(&confirmation.rotation_id)
            .ok_or_else(|| {
                sync_protocol_error(
                    "epochs",
                    format!("Rotation not found: {}", confirmation.rotation_id),
                )
            })?;

        // Validate confirmation
        if confirmation.ready_for_epoch != rotation.target_epoch {
            return Err(sync_protocol_error(
                "epochs",
                format!(
                    "Epoch mismatch: expected {}, got {}",
                    rotation.target_epoch, confirmation.ready_for_epoch
                ),
            ));
        }

        // Store confirmation
        rotation
            .confirmations
            .insert(confirmation.participant_id, confirmation);

        // Check if we have enough confirmations
        let ready_for_commit = rotation.confirmations.len() >= self.epoch_config.rotation_threshold;

        if ready_for_commit {
            rotation.status = RotationStatus::Synchronizing;
        }

        Ok(ready_for_commit)
    }

    /// Commit epoch rotation
    pub fn commit_rotation(&mut self, rotation_id: &str) -> Result<u64, SyncError> {
        let rotation = self
            .pending_rotations
            .get_mut(rotation_id)
            .ok_or_else(|| SyncError::not_found(format!("Rotation not found: {}", rotation_id)))?;

        if rotation.status != RotationStatus::Synchronizing {
            return Err(sync_protocol_error(
                "epochs",
                format!("Invalid rotation status: {:?}", rotation.status),
            ));
        }

        // Commit the epoch
        self.current_epoch = rotation.target_epoch;
        rotation.status = RotationStatus::Completed;

        Ok(self.current_epoch)
    }

    /// Clean up completed rotations
    pub fn cleanup_completed_rotations(&mut self) -> usize {
        let initial_count = self.pending_rotations.len();

        self.pending_rotations.retain(|_, rotation| {
            !matches!(
                rotation.status,
                RotationStatus::Completed | RotationStatus::Failed(_)
            )
        });

        initial_count - self.pending_rotations.len()
    }

    /// Get rotation status
    pub fn get_rotation_status(&self, rotation_id: &str) -> Option<&RotationStatus> {
        self.pending_rotations.get(rotation_id).map(|r| &r.status)
    }

    /// List pending rotations
    pub fn list_pending_rotations(&self) -> Vec<&EpochRotation> {
        self.pending_rotations.values().collect()
    }
}

// Choreographic Protocol Definition
// Coordinated epoch rotation across multiple participants
// NOTE: Choreography block commented out to maintain aura-sync's semantic independence.
// This protocol should be implemented using pure effect composition instead of macros.
//
// BISCUIT INTEGRATION: When re-enabling choreography, these guard_capability annotations
// are compatible with BiscuitGuardEvaluator for token-based authorization.
/*
choreography! {
    #[namespace = "epoch_rotation"]
    protocol EpochRotationProtocol {
        roles: Coordinator, Participant1, Participant2;

        // Phase 1: Propose epoch rotation
        Coordinator[guard_capability = "propose_epoch_rotation",
                   flow_cost = 120,
                   journal_facts = "epoch_rotation_proposed"]
        -> Participant1: EpochRotationProposal(EpochRotationProposal);

        Coordinator[guard_capability = "propose_epoch_rotation",
                   flow_cost = 120]
        -> Participant2: EpochRotationProposal(EpochRotationProposal);

        // Phase 2: Participants confirm readiness
        Participant1[guard_capability = "confirm_epoch_readiness",
                    flow_cost = 80,
                    journal_facts = "epoch_confirmation_sent"]
        -> Coordinator: EpochConfirmation(EpochConfirmation);

        Participant2[guard_capability = "confirm_epoch_readiness",
                    flow_cost = 80,
                    journal_facts = "epoch_confirmation_sent"]
        -> Coordinator: EpochConfirmation(EpochConfirmation);

        // Phase 3: Synchronized commit
        Coordinator[guard_capability = "commit_epoch_rotation",
                   flow_cost = 100,
                   journal_facts = "epoch_rotation_committed"]
        -> Participant1: EpochCommit(EpochCommit);

        Coordinator[guard_capability = "commit_epoch_rotation",
                   flow_cost = 100,
                   journal_facts = "epoch_rotation_committed"]
        -> Participant2: EpochCommit(EpochCommit);
    }
}
*/
