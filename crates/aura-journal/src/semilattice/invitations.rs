//! Invitation ledger semilattice used by aura-invitation.

use aura_core::semilattice::{Bottom, JoinSemilattice};
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

/// Ledger record for a specific invitation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InvitationRecord {
    /// Invitation identifier.
    pub invitation_id: String,
    /// Current ledger status.
    pub status: InvitationStatus,
    /// Timestamp of the last update.
    pub updated_at: u64,
    /// Expiry timestamp.
    pub expires_at: u64,
}

impl InvitationRecord {
    /// Create a pending record.
    pub fn pending(invitation_id: impl Into<String>, expires_at: u64, timestamp: u64) -> Self {
        Self {
            invitation_id: invitation_id.into(),
            status: InvitationStatus::Pending,
            updated_at: timestamp,
            expires_at,
        }
    }

    /// Update status while retaining metadata.
    pub fn set_status(&mut self, status: InvitationStatus, timestamp: u64) {
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

/// Ledger storing invitation records with join semantics.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct InvitationLedger {
    entries: BTreeMap<String, InvitationRecord>,
}

impl InvitationLedger {
    /// Create an empty ledger.
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
    pub fn mark_accepted(&mut self, invitation_id: &str, timestamp: u64) {
        if let Some(record) = self.entries.get_mut(invitation_id) {
            record.set_status(InvitationStatus::Accepted, timestamp);
        }
    }

    /// Mark a record as expired.
    pub fn mark_expired(&mut self, invitation_id: &str, timestamp: u64) {
        if let Some(record) = self.entries.get_mut(invitation_id) {
            record.set_status(InvitationStatus::Expired, timestamp);
        }
    }
}

impl JoinSemilattice for InvitationLedger {
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

impl Bottom for InvitationLedger {
    fn bottom() -> Self {
        Self::new()
    }
}
