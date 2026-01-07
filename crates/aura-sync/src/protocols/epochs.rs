//! Epoch Management Pattern
//!
//! Generic epoch rotation coordination pattern.
//!
//! **Time System**: Uses `PhysicalTime` for timestamps per the unified time architecture.

use crate::core::sync_protocol_error;
use aura_core::time::PhysicalTime;
use aura_core::types::Epoch;
use aura_core::{AuraError, ContextId, DeviceId};
use aura_macros::choreography;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;

/// Epoch rotation coordinator using choreographic protocols
#[derive(Debug, Clone)]
pub struct EpochRotationCoordinator {
    #[allow(dead_code)]
    device_id: DeviceId,
    current_epoch: Epoch,
    epoch_config: EpochConfig,
    pending_rotations: HashMap<String, EpochRotation>,
}

/// Configuration for epoch management
#[derive(Debug, Clone)]
pub struct EpochConfig {
    pub epoch_duration: Duration,
    pub rotation_threshold: u32, // Minimum participants for rotation
    pub synchronization_timeout: Duration,
}

/// Epoch rotation state
///
/// **Time System**: Uses `PhysicalTime` for timestamps per the unified time architecture.
#[derive(Debug, Clone)]
pub struct EpochRotation {
    pub rotation_id: String,
    pub target_epoch: Epoch,
    pub participants: Vec<DeviceId>,
    pub confirmations: HashMap<DeviceId, EpochConfirmation>,
    /// When rotation was initiated (unified time system)
    pub initiated_at: PhysicalTime,
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
///
/// **Time System**: Uses `PhysicalTime` for timestamps per the unified time architecture.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EpochRotationProposal {
    pub rotation_id: String,
    pub proposed_epoch: Epoch,
    pub proposer_id: DeviceId,
    pub participants: Vec<DeviceId>,
    pub context_id: ContextId,
    /// When proposal was created (unified time system)
    pub timestamp: PhysicalTime,
}

/// Epoch confirmation from participant
///
/// **Time System**: Uses `PhysicalTime` for timestamps per the unified time architecture.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EpochConfirmation {
    pub rotation_id: String,
    pub participant_id: DeviceId,
    pub current_epoch: Epoch,
    pub ready_for_epoch: Epoch,
    /// When confirmation was sent (unified time system)
    pub confirmation_timestamp: PhysicalTime,
}

/// Synchronized epoch commit
///
/// **Time System**: Uses `PhysicalTime` for timestamps per the unified time architecture.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EpochCommit {
    pub rotation_id: String,
    pub committed_epoch: Epoch,
    /// When commit was issued (unified time system)
    pub commit_timestamp: PhysicalTime,
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
    pub fn new(device_id: DeviceId, initial_epoch: Epoch, config: EpochConfig) -> Self {
        Self {
            device_id,
            current_epoch: initial_epoch,
            epoch_config: config,
            pending_rotations: HashMap::new(),
        }
    }

    /// Get current epoch
    pub fn current_epoch(&self) -> Epoch {
        self.current_epoch
    }

    /// Check if epoch rotation is needed
    ///
    /// **Time System**: Uses `PhysicalTime` for timestamps.
    /// Compares current time with last rotation time to determine if epoch duration has elapsed.
    pub fn needs_rotation(
        &self,
        last_rotation: &PhysicalTime,
        current_time: &PhysicalTime,
    ) -> bool {
        let elapsed_ms = current_time.ts_ms.saturating_sub(last_rotation.ts_ms);
        elapsed_ms >= self.epoch_config.epoch_duration.as_millis() as u64
    }

    /// Initiate epoch rotation
    ///
    /// **Time System**: Uses `PhysicalTime` for timestamps.
    pub fn initiate_rotation(
        &mut self,
        participants: Vec<DeviceId>,
        _context_id: ContextId,
        now: &PhysicalTime,
    ) -> Result<String, AuraError> {
        let participants_len = participants.len() as u32;
        if participants_len < self.epoch_config.rotation_threshold {
            return Err(sync_protocol_error(
                "epochs",
                format!(
                    "Insufficient participants: {} < {}",
                    participants_len, self.epoch_config.rotation_threshold
                ),
            ));
        }

        let target_epoch = self
            .current_epoch
            .next()
            .map_err(|e| sync_protocol_error("epochs", format!("epoch overflow: {e}")))?;
        let rotation_id = format!("epoch-rotation-{}-{}", target_epoch.value(), now.ts_ms);

        let rotation = EpochRotation {
            rotation_id: rotation_id.clone(),
            target_epoch,
            participants,
            confirmations: HashMap::new(),
            initiated_at: now.clone(),
            status: RotationStatus::Initiated,
        };

        self.pending_rotations.insert(rotation_id.clone(), rotation);
        Ok(rotation_id)
    }

    /// Process epoch confirmation
    pub fn process_confirmation(
        &mut self,
        confirmation: EpochConfirmation,
    ) -> Result<bool, AuraError> {
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
        let confirmations_len = rotation.confirmations.len() as u32;
        let ready_for_commit = confirmations_len >= self.epoch_config.rotation_threshold;

        if ready_for_commit {
            rotation.status = RotationStatus::Synchronizing;
        }

        Ok(ready_for_commit)
    }

    /// Commit epoch rotation
    pub fn commit_rotation(&mut self, rotation_id: &str) -> Result<Epoch, AuraError> {
        let rotation = self
            .pending_rotations
            .get_mut(rotation_id)
            .ok_or_else(|| AuraError::not_found(format!("Rotation not found: {rotation_id}")))?;

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
choreography!(include_str!("src/protocols/epochs.choreo"));
