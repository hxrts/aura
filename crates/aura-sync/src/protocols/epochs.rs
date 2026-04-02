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
use thiserror::Error;

/// Epoch rotation coordinator using choreographic protocols
#[derive(Debug, Clone)]
pub struct EpochRotationCoordinator {
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
    Failed(RotationFailure),
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum RotationFailure {
    #[error("insufficient participants: {actual} < {required}")]
    InsufficientParticipants { actual: u32, required: u32 },
    #[error("epoch overflow: {reason}")]
    EpochOverflow { reason: String },
    #[error("rotation not found: {rotation_id}")]
    RotationNotFound { rotation_id: String },
    #[error("epoch mismatch: expected {expected}, got {actual}")]
    EpochMismatch { expected: Epoch, actual: Epoch },
    #[error("invalid rotation status: {status:?}")]
    InvalidStatus { status: RotationPhase },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RotationPhase {
    Initiated,
    Gathering,
    Synchronizing,
    Completed,
    Failed,
}

impl RotationStatus {
    pub fn phase(&self) -> RotationPhase {
        match self {
            RotationStatus::Initiated => RotationPhase::Initiated,
            RotationStatus::Gathering => RotationPhase::Gathering,
            RotationStatus::Synchronizing => RotationPhase::Synchronizing,
            RotationStatus::Completed => RotationPhase::Completed,
            RotationStatus::Failed(_) => RotationPhase::Failed,
        }
    }
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
            let failure = RotationFailure::InsufficientParticipants {
                actual: participants_len,
                required: self.epoch_config.rotation_threshold,
            };
            return Err(sync_protocol_error("epochs", failure.to_string()));
        }

        let target_epoch = self.current_epoch.next().map_err(|e| {
            let failure = RotationFailure::EpochOverflow {
                reason: e.to_string(),
            };
            sync_protocol_error("epochs", failure.to_string())
        })?;
        let rotation_id = format!(
            "epoch-rotation-{}-{}-{}",
            self.device_id,
            target_epoch.value(),
            now.ts_ms
        );

        let rotation = EpochRotation {
            rotation_id: rotation_id.clone(),
            target_epoch,
            participants,
            confirmations: HashMap::new(),
            initiated_at: now.clone(),
            status: RotationStatus::Gathering,
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
                let failure = RotationFailure::RotationNotFound {
                    rotation_id: confirmation.rotation_id.clone(),
                };
                sync_protocol_error("epochs", failure.to_string())
            })?;

        // Validate confirmation
        if confirmation.ready_for_epoch != rotation.target_epoch {
            let failure = RotationFailure::EpochMismatch {
                expected: rotation.target_epoch,
                actual: confirmation.ready_for_epoch,
            };
            rotation.status = RotationStatus::Failed(failure.clone());
            return Err(sync_protocol_error("epochs", failure.to_string()));
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
        let rotation = self.pending_rotations.get_mut(rotation_id).ok_or_else(|| {
            let failure = RotationFailure::RotationNotFound {
                rotation_id: rotation_id.to_string(),
            };
            sync_protocol_error("epochs", failure.to_string())
        })?;

        if rotation.status != RotationStatus::Synchronizing {
            let failure = RotationFailure::InvalidStatus {
                status: rotation.status.phase(),
            };
            rotation.status = RotationStatus::Failed(failure.clone());
            return Err(sync_protocol_error("epochs", failure.to_string()));
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
// The generated manifest carries epoch-rotation transfer link metadata for reconfiguration.
// Runtime reconfiguration still consumes the epoch_rotation_transfer bundle
// contract exposed by this choreography surface.
choreography!(include_str!("src/protocols/epochs.tell"));

#[cfg(test)]
mod tests {
    use super::*;

    fn device(seed: u8) -> DeviceId {
        DeviceId::new_from_entropy([seed; 32])
    }

    fn time(ts_ms: u64) -> PhysicalTime {
        PhysicalTime {
            ts_ms,
            uncertainty: None,
        }
    }

    #[test]
    #[allow(clippy::expect_used)]
    fn confirmation_mismatch_marks_rotation_failed() {
        let context_id = ContextId::new_from_entropy([1u8; 32]);
        let mut coordinator =
            EpochRotationCoordinator::new(device(1), Epoch::new(0), EpochConfig::default());
        let rotation_id = coordinator
            .initiate_rotation(vec![device(2), device(3)], context_id, &time(100))
            .expect("rotation should be created");

        let error = coordinator
            .process_confirmation(EpochConfirmation {
                rotation_id: rotation_id.clone(),
                participant_id: device(2),
                current_epoch: Epoch::new(0),
                ready_for_epoch: Epoch::new(9),
                confirmation_timestamp: time(101),
            })
            .expect_err("mismatched epoch should fail");

        assert!(error.to_string().contains("epoch mismatch"));
        assert_eq!(
            coordinator.get_rotation_status(&rotation_id),
            Some(&RotationStatus::Failed(RotationFailure::EpochMismatch {
                expected: Epoch::new(1),
                actual: Epoch::new(9),
            }))
        );
    }

    #[test]
    #[allow(clippy::expect_used)]
    fn invalid_commit_marks_rotation_failed() {
        let context_id = ContextId::new_from_entropy([2u8; 32]);
        let mut coordinator =
            EpochRotationCoordinator::new(device(4), Epoch::new(0), EpochConfig::default());
        let rotation_id = coordinator
            .initiate_rotation(vec![device(5), device(6)], context_id, &time(200))
            .expect("rotation should be created");

        let error = coordinator
            .commit_rotation(&rotation_id)
            .expect_err("commit before synchronization should fail");

        assert!(error.to_string().contains("invalid rotation status"));
        assert_eq!(
            coordinator.get_rotation_status(&rotation_id),
            Some(&RotationStatus::Failed(RotationFailure::InvalidStatus {
                status: RotationPhase::Gathering,
            }))
        );
    }
}
