//! # Recovery View State

use serde::{Deserialize, Serialize};

/// Guardian status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
pub enum GuardianStatus {
    /// Guardian is active and can participate in recovery
    #[default]
    Active,
    /// Guardian invitation is pending
    Pending,
    /// Guardian has been revoked
    Revoked,
    /// Guardian is offline/unreachable
    Offline,
}

/// A guardian
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct Guardian {
    /// Guardian identifier
    pub id: String,
    /// Guardian display name (petname)
    pub name: String,
    /// Guardian status
    pub status: GuardianStatus,
    /// When this guardian was added (ms since epoch)
    pub added_at: u64,
    /// Last seen time (ms since epoch)
    pub last_seen: Option<u64>,
}

/// Recovery process status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
pub enum RecoveryProcessStatus {
    /// No recovery in progress
    #[default]
    Idle,
    /// Recovery has been initiated
    Initiated,
    /// Waiting for guardian approvals
    WaitingForApprovals,
    /// Recovery approved, executing
    Approved,
    /// Recovery completed
    Completed,
    /// Recovery failed or was rejected
    Failed,
}

/// Active recovery process (if any)
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct RecoveryProcess {
    /// Recovery context ID
    pub id: String,
    /// Current status
    pub status: RecoveryProcessStatus,
    /// Number of approvals received
    pub approvals_received: u32,
    /// Number of approvals required
    pub approvals_required: u32,
    /// Guardian IDs that have approved
    pub approved_by: Vec<String>,
    /// When recovery was initiated (ms since epoch)
    pub initiated_at: u64,
    /// When recovery expires (ms since epoch)
    pub expires_at: Option<u64>,
}

/// Recovery state
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct RecoveryState {
    /// All guardians
    pub guardians: Vec<Guardian>,
    /// Current threshold (M of N)
    pub threshold: u32,
    /// Total guardian count
    pub guardian_count: u32,
    /// Active recovery process (if any)
    pub active_recovery: Option<RecoveryProcess>,
    /// Recovery requests for accounts we're a guardian of
    pub pending_requests: Vec<RecoveryProcess>,
}

impl RecoveryState {
    /// Check if recovery is possible (enough active guardians)
    pub fn can_recover(&self) -> bool {
        let active_count = self
            .guardians
            .iter()
            .filter(|g| g.status == GuardianStatus::Active)
            .count() as u32;
        active_count >= self.threshold
    }

    /// Get guardian by ID
    pub fn guardian(&self, id: &str) -> Option<&Guardian> {
        self.guardians.iter().find(|g| g.id == id)
    }

    /// Initiate a recovery process
    pub fn initiate_recovery(&mut self, session_id: String, initiated_at: u64) {
        self.active_recovery = Some(RecoveryProcess {
            id: session_id,
            status: RecoveryProcessStatus::Initiated,
            approvals_received: 0,
            approvals_required: self.threshold,
            approved_by: Vec::new(),
            initiated_at,
            expires_at: None,
        });
    }

    /// Add a guardian approval to the active recovery
    pub fn add_guardian_approval(&mut self, guardian_id: String) {
        if let Some(ref mut recovery) = self.active_recovery {
            if !recovery.approved_by.contains(&guardian_id) {
                recovery.approved_by.push(guardian_id);
                recovery.approvals_received += 1;

                // Update status based on approvals
                if recovery.approvals_received >= recovery.approvals_required {
                    recovery.status = RecoveryProcessStatus::Approved;
                } else {
                    recovery.status = RecoveryProcessStatus::WaitingForApprovals;
                }
            }
        }
    }

    /// Toggle guardian status for a contact
    ///
    /// If is_guardian is true, adds/activates the guardian.
    /// If is_guardian is false, removes/revokes the guardian.
    pub fn toggle_guardian(&mut self, contact_id: String, is_guardian: bool) {
        if is_guardian {
            // Check if guardian already exists
            if let Some(guardian) = self.guardians.iter_mut().find(|g| g.id == contact_id) {
                // Reactivate existing guardian
                guardian.status = GuardianStatus::Active;
            } else {
                // Add new guardian
                self.guardians.push(Guardian {
                    id: contact_id,
                    name: String::new(), // Will be resolved from contacts
                    status: GuardianStatus::Active,
                    added_at: 0, // Timestamp would come from fact
                    last_seen: None,
                });
                self.guardian_count += 1;
            }
        } else {
            // Revoke guardian status
            if let Some(guardian) = self.guardians.iter_mut().find(|g| g.id == contact_id) {
                guardian.status = GuardianStatus::Revoked;
                // Note: We don't remove from list to preserve history
            }
        }
    }

    /// Set the recovery threshold
    pub fn set_threshold(&mut self, threshold: u32) {
        self.threshold = threshold;
    }
}
