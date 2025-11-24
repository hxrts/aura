//! Invitation registry semilattice used by aura-invitation.

use aura_core::semilattice::{Bottom, JoinSemilattice};
use aura_core::time::TimeStamp;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Status transitions for invitations.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub enum InvitationStatus {
    /// Invitation was created and waits for acceptance.
    Pending,
    /// Invitation was accepted and should not be reused.
    Accepted,
    /// Invitation expired locally.
    Expired,
}

impl InvitationStatus {
    fn priority(self) -> u8 {
        match self {
            InvitationStatus::Pending => 0,
            InvitationStatus::Expired => 1,
            InvitationStatus::Accepted => 2,
        }
    }

    fn merge(self, other: Self) -> Self {
        if self.priority() >= other.priority() {
            self
        } else {
            other
        }
    }
}

/// Registry record for a specific invitation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InvitationRecord {
    /// Invitation identifier.
    pub invitation_id: String,
    /// Current registry status.
    pub status: InvitationStatus,
    /// Time of the last update (using unified time system).
    pub updated_at: TimeStamp,
    /// Expiry time (using unified time system).
    pub expires_at: TimeStamp,
}

impl InvitationRecord {
    /// Create a pending record.
    pub fn pending(
        invitation_id: impl Into<String>,
        expires_at: TimeStamp,
        updated_at: TimeStamp,
    ) -> Self {
        Self {
            invitation_id: invitation_id.into(),
            status: InvitationStatus::Pending,
            updated_at,
            expires_at,
        }
    }

    /// Update status while retaining metadata.
    pub fn set_status(&mut self, status: InvitationStatus, timestamp: TimeStamp) {
        if self.status.merge(status) != self.status {
            self.status = status;
            self.updated_at = timestamp;
        }
    }
}

impl JoinSemilattice for InvitationRecord {
    fn join(&self, other: &Self) -> Self {
        if self.invitation_id != other.invitation_id {
            return self.clone();
        }

        if self.status.priority() >= other.status.priority() {
            self.clone()
        } else {
            other.clone()
        }
    }
}

/// Registry storing invitation records with join semantics.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct InvitationRecordRegistry {
    entries: BTreeMap<String, InvitationRecord>,
}

impl InvitationRecordRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self {
            entries: BTreeMap::new(),
        }
    }

    /// Insert or update a record.
    pub fn upsert(&mut self, record: InvitationRecord) {
        self.entries
            .entry(record.invitation_id.clone())
            .and_modify(|existing| *existing = existing.join(&record))
            .or_insert(record);
    }

    /// Lookup a record by identifier.
    pub fn get(&self, invitation_id: &str) -> Option<&InvitationRecord> {
        self.entries.get(invitation_id)
    }

    /// Mark a record as accepted.
    pub fn mark_accepted(&mut self, invitation_id: &str, timestamp: TimeStamp) {
        if let Some(record) = self.entries.get_mut(invitation_id) {
            record.set_status(InvitationStatus::Accepted, timestamp);
        }
    }

    /// Mark a record as expired.
    pub fn mark_expired(&mut self, invitation_id: &str, timestamp: TimeStamp) {
        if let Some(record) = self.entries.get_mut(invitation_id) {
            record.set_status(InvitationStatus::Expired, timestamp);
        }
    }

    /// Get the number of records in the registry.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Check if the registry is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

impl JoinSemilattice for InvitationRecordRegistry {
    fn join(&self, other: &Self) -> Self {
        let mut merged = self.entries.clone();
        for (id, record) in &other.entries {
            merged
                .entry(id.clone())
                .and_modify(|existing| *existing = existing.join(record))
                .or_insert(record.clone());
        }
        Self { entries: merged }
    }
}

impl Bottom for InvitationRecordRegistry {
    fn bottom() -> Self {
        Self::new()
    }
}
