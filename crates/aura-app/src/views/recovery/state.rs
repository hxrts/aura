//! Recovery-state entities and state container.

use super::errors::RecoveryError;
use aura_core::types::identifiers::{AuthorityId, CeremonyId, ContextId};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
pub enum GuardianStatus {
    #[default]
    Active,
    Pending,
    Revoked,
    Offline,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct Guardian {
    pub id: AuthorityId,
    pub name: String,
    pub status: GuardianStatus,
    pub added_at: u64,
    pub last_seen: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct GuardianBinding {
    pub account_authority: AuthorityId,
    pub context_id: ContextId,
    pub bound_at: u64,
    pub account_name: Option<String>,
}

impl GuardianBinding {
    #[must_use]
    pub fn new(account_authority: AuthorityId, context_id: ContextId, bound_at: u64) -> Self {
        Self {
            account_authority,
            context_id,
            bound_at,
            account_name: None,
        }
    }

    #[must_use]
    pub fn with_name(
        account_authority: AuthorityId,
        context_id: ContextId,
        bound_at: u64,
        account_name: impl Into<String>,
    ) -> Self {
        Self {
            account_authority,
            context_id,
            bound_at,
            account_name: Some(account_name.into()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
pub enum RecoveryProcessStatus {
    #[default]
    Idle,
    Initiated,
    WaitingForApprovals,
    Approved,
    Completed,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct RecoveryApproval {
    pub guardian_id: AuthorityId,
    pub approved_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct RecoveryProcess {
    pub id: CeremonyId,
    pub account_id: AuthorityId,
    pub status: RecoveryProcessStatus,
    pub approvals_received: u32,
    pub approvals_required: u32,
    pub approved_by: Vec<AuthorityId>,
    pub approvals: Vec<RecoveryApproval>,
    pub initiated_at: u64,
    pub expires_at: Option<u64>,
    pub progress: u32,
}

impl RecoveryProcess {
    pub fn is_threshold_met(&self) -> bool {
        self.approvals_received >= self.approvals_required
    }

    pub fn progress_fraction(&self) -> f64 {
        if self.approvals_required == 0 {
            return 1.0;
        }
        f64::from(self.approvals_received) / f64::from(self.approvals_required)
    }

    pub fn has_guardian_approved(&self, guardian_id: &AuthorityId) -> bool {
        self.approved_by.contains(guardian_id)
    }

    pub fn can_complete(&self) -> bool {
        self.is_threshold_met()
            && self.status != RecoveryProcessStatus::Failed
            && self.status != RecoveryProcessStatus::Completed
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct RecoveryState {
    #[serde(with = "guardian_map_serde")]
    guardians: HashMap<AuthorityId, Guardian>,
    threshold: u32,
    active_recovery: Option<RecoveryProcess>,
    pending_requests: Vec<RecoveryProcess>,
    guardian_bindings: Vec<GuardianBinding>,
}

mod guardian_map_serde {
    use super::*;
    use serde::{Deserializer, Serializer};

    pub fn serialize<S>(
        map: &HashMap<AuthorityId, Guardian>,
        serializer: S,
    ) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let vec: Vec<&Guardian> = map.values().collect();
        vec.serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<HashMap<AuthorityId, Guardian>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let vec: Vec<Guardian> = Vec::deserialize(deserializer)?;
        Ok(vec
            .into_iter()
            .map(|guardian| (guardian.id, guardian))
            .collect())
    }
}

impl RecoveryState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn from_parts(
        guardians: impl IntoIterator<Item = Guardian>,
        threshold: u32,
        active_recovery: Option<RecoveryProcess>,
        pending_requests: Vec<RecoveryProcess>,
        guardian_bindings: Vec<GuardianBinding>,
    ) -> Self {
        Self {
            guardians: guardians
                .into_iter()
                .map(|guardian| (guardian.id, guardian))
                .collect(),
            threshold,
            active_recovery,
            pending_requests,
            guardian_bindings,
        }
    }

    pub fn can_recover(&self) -> bool {
        let active_count = self
            .guardians
            .values()
            .filter(|guardian| guardian.status == GuardianStatus::Active)
            .count() as u32;
        active_count >= self.threshold
    }

    pub fn guardian(&self, id: &AuthorityId) -> Option<&Guardian> {
        self.guardians.get(id)
    }

    pub fn guardian_mut(&mut self, id: &AuthorityId) -> Option<&mut Guardian> {
        self.guardians.get_mut(id)
    }

    pub fn all_guardians(&self) -> impl Iterator<Item = &Guardian> {
        self.guardians.values()
    }

    pub fn guardian_ids(&self) -> impl Iterator<Item = &AuthorityId> {
        self.guardians.keys()
    }

    pub fn guardian_count(&self) -> usize {
        self.guardians.len()
    }

    pub fn has_guardian(&self, id: &AuthorityId) -> bool {
        self.guardians.contains_key(id)
    }

    pub fn active_guardians(&self) -> impl Iterator<Item = &Guardian> {
        self.guardians
            .values()
            .filter(|guardian| guardian.status == GuardianStatus::Active)
    }

    pub fn threshold(&self) -> u32 {
        self.threshold
    }

    pub fn active_recovery(&self) -> Option<&RecoveryProcess> {
        self.active_recovery.as_ref()
    }

    pub fn active_recovery_mut(&mut self) -> Option<&mut RecoveryProcess> {
        self.active_recovery.as_mut()
    }

    pub fn pending_requests(&self) -> &[RecoveryProcess] {
        &self.pending_requests
    }

    pub fn pending_requests_mut(&mut self) -> &mut Vec<RecoveryProcess> {
        &mut self.pending_requests
    }

    pub fn initiate_recovery(
        &mut self,
        session_id: CeremonyId,
        account_id: AuthorityId,
        initiated_at: u64,
    ) {
        self.active_recovery = Some(RecoveryProcess {
            id: session_id,
            account_id,
            status: RecoveryProcessStatus::Initiated,
            approvals_received: 0,
            approvals_required: self.threshold,
            approved_by: Vec::new(),
            approvals: Vec::new(),
            initiated_at,
            expires_at: None,
            progress: 0,
        });
    }

    pub fn add_guardian_approval(&mut self, guardian_id: AuthorityId) -> Result<(), RecoveryError> {
        self.add_guardian_approval_with_timestamp(guardian_id, 0)
    }

    pub fn add_guardian_approval_with_timestamp(
        &mut self,
        guardian_id: AuthorityId,
        timestamp: u64,
    ) -> Result<(), RecoveryError> {
        let recovery = self
            .active_recovery
            .as_mut()
            .ok_or(RecoveryError::NoActiveRecovery)?;

        if recovery.approved_by.contains(&guardian_id) {
            return Err(RecoveryError::AlreadyApproved(guardian_id));
        }

        recovery.approved_by.push(guardian_id);
        recovery.approvals.push(RecoveryApproval {
            guardian_id,
            approved_at: timestamp,
        });
        recovery.approvals_received += 1;

        if recovery.approvals_required > 0 {
            recovery.progress = (recovery.approvals_received * 100) / recovery.approvals_required;
        }

        if recovery.approvals_received >= recovery.approvals_required {
            recovery.status = RecoveryProcessStatus::Approved;
            recovery.progress = 100;
        } else {
            recovery.status = RecoveryProcessStatus::WaitingForApprovals;
        }

        Ok(())
    }

    pub fn complete_recovery(&mut self) -> Result<(), RecoveryError> {
        let recovery = self
            .active_recovery
            .as_mut()
            .ok_or(RecoveryError::NoActiveRecovery)?;
        recovery.status = RecoveryProcessStatus::Completed;
        recovery.progress = 100;
        Ok(())
    }

    pub fn fail_recovery(&mut self) -> Result<(), RecoveryError> {
        let recovery = self
            .active_recovery
            .as_mut()
            .ok_or(RecoveryError::NoActiveRecovery)?;
        recovery.status = RecoveryProcessStatus::Failed;
        Ok(())
    }

    pub fn clear_recovery(&mut self) {
        self.active_recovery = None;
    }

    pub fn set_active_recovery(&mut self, recovery: Option<RecoveryProcess>) {
        self.active_recovery = recovery;
    }

    pub fn apply_guardian(&mut self, guardian: Guardian) -> Result<(), RecoveryError> {
        if self.guardians.contains_key(&guardian.id) {
            return Err(RecoveryError::GuardianAlreadyExists(guardian.id));
        }
        self.guardians.insert(guardian.id, guardian);
        Ok(())
    }

    pub fn upsert_guardian(&mut self, guardian: Guardian) {
        self.guardians.insert(guardian.id, guardian);
    }

    pub fn update_guardian(
        &mut self,
        id: &AuthorityId,
        f: impl FnOnce(&mut Guardian),
    ) -> Result<(), RecoveryError> {
        let guardian = self
            .guardians
            .get_mut(id)
            .ok_or(RecoveryError::GuardianNotFound(*id))?;
        f(guardian);
        Ok(())
    }

    pub fn remove_guardian(&mut self, id: &AuthorityId) -> Option<Guardian> {
        self.guardians.remove(id)
    }

    pub fn revoke_guardian(&mut self, id: &AuthorityId) -> Result<(), RecoveryError> {
        self.update_guardian(id, |guardian| guardian.status = GuardianStatus::Revoked)
    }

    pub fn activate_guardian(&mut self, id: &AuthorityId) -> Result<(), RecoveryError> {
        self.update_guardian(id, |guardian| guardian.status = GuardianStatus::Active)
    }

    pub fn set_threshold(&mut self, threshold: u32) {
        self.threshold = threshold;
    }

    pub fn retain_guardians(&mut self, ids: &[AuthorityId]) {
        self.guardians.retain(|id, _| ids.contains(id));
    }

    pub fn clear_guardians(&mut self) {
        self.guardians.clear();
    }

    pub fn is_guardian_for(&self, account: &AuthorityId) -> bool {
        self.guardian_bindings
            .iter()
            .any(|binding| binding.account_authority == *account)
    }

    pub fn add_guardian_for(&mut self, account: AuthorityId, context_id: ContextId, bound_at: u64) {
        if !self.is_guardian_for(&account) {
            self.guardian_bindings
                .push(GuardianBinding::new(account, context_id, bound_at));
        }
    }

    pub fn add_guardian_for_with_name(
        &mut self,
        account: AuthorityId,
        context_id: ContextId,
        bound_at: u64,
        account_name: impl Into<String>,
    ) {
        if !self.is_guardian_for(&account) {
            self.guardian_bindings.push(GuardianBinding::with_name(
                account,
                context_id,
                bound_at,
                account_name,
            ));
        }
    }

    pub fn remove_guardian_for(&mut self, account: &AuthorityId) {
        self.guardian_bindings
            .retain(|binding| binding.account_authority != *account);
    }

    pub fn guardian_binding_for(&self, account: &AuthorityId) -> Option<&GuardianBinding> {
        self.guardian_bindings
            .iter()
            .find(|binding| binding.account_authority == *account)
    }

    pub fn guardian_binding_count(&self) -> usize {
        self.guardian_bindings.len()
    }
}

#[must_use]
pub fn format_recovery_status(active: &[String], completed: &[String]) -> String {
    use std::fmt::Write;

    let mut output = String::new();

    if active.is_empty() {
        let _ = writeln!(output, "No active recovery sessions found.");
    } else {
        let _ = writeln!(output, "Found {} active recovery session(s):", active.len());
        for (index, key) in active.iter().enumerate() {
            let _ = writeln!(output, "  {}. {}", index + 1, key);
        }
    }

    if !completed.is_empty() {
        let _ = writeln!(output, "Completed recovery sessions ({}):", completed.len());
        for key in completed {
            let _ = writeln!(output, "  - {key}");
        }
    }

    output
}
