//! # Recovery View State

use aura_core::identifiers::{AuthorityId, ContextId};
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
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct Guardian {
    /// Guardian identifier
    pub id: AuthorityId,
    /// Guardian display name (nickname)
    pub name: String,
    /// Guardian status
    pub status: GuardianStatus,
    /// When this guardian was added (ms since epoch)
    pub added_at: u64,
    /// Last seen time (ms since epoch)
    pub last_seen: Option<u64>,
}

/// A guardian binding - represents our role as guardian for another account.
///
/// This tracks accounts we are a guardian for (we can approve their recovery).
/// This is distinct from `Guardian` which tracks guardians of *our* account.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct GuardianBinding {
    /// The account we are guardian for
    pub account_authority: AuthorityId,
    /// The relational context for this guardian binding
    pub context_id: ContextId,
    /// When this binding was established (ms since epoch)
    pub bound_at: u64,
    /// Display name for the account (if known)
    pub account_name: Option<String>,
}

impl GuardianBinding {
    /// Create a new guardian binding.
    #[must_use]
    pub fn new(
        account_authority: AuthorityId,
        context_id: ContextId,
        bound_at: u64,
    ) -> Self {
        Self {
            account_authority,
            context_id,
            bound_at,
            account_name: None,
        }
    }

    /// Create a new guardian binding with account name.
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

/// Guardian approval for recovery (with detailed info)
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct RecoveryApproval {
    /// Guardian ID who approved
    pub guardian_id: AuthorityId,
    /// Timestamp when approved (ms since epoch)
    pub approved_at: u64,
}

/// Active recovery process (if any)
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct RecoveryProcess {
    /// Recovery context ID
    pub id: String,
    /// Account being recovered
    pub account_id: AuthorityId,
    /// Current status
    pub status: RecoveryProcessStatus,
    /// Number of approvals received
    pub approvals_received: u32,
    /// Number of approvals required
    pub approvals_required: u32,
    /// Guardian IDs that have approved
    pub approved_by: Vec<AuthorityId>,
    /// Detailed approval records
    pub approvals: Vec<RecoveryApproval>,
    /// When recovery was initiated (ms since epoch)
    pub initiated_at: u64,
    /// When recovery expires (ms since epoch)
    pub expires_at: Option<u64>,
    /// Progress percentage (0-100)
    pub progress: u32,
}

impl RecoveryProcess {
    /// Check if the approval threshold has been met
    ///
    /// Returns true when enough guardians have approved the recovery.
    pub fn is_threshold_met(&self) -> bool {
        self.approvals_received >= self.approvals_required
    }

    /// Get progress as a fraction (0.0 to 1.0)
    ///
    /// Useful for UI progress bars.
    pub fn progress_fraction(&self) -> f64 {
        if self.approvals_required == 0 {
            return 1.0;
        }
        f64::from(self.approvals_received) / f64::from(self.approvals_required)
    }

    /// Check if this guardian has already approved
    pub fn has_guardian_approved(&self, guardian_id: &AuthorityId) -> bool {
        self.approved_by.contains(guardian_id)
    }

    /// Check if recovery can be completed (threshold met and not failed)
    pub fn can_complete(&self) -> bool {
        self.is_threshold_met()
            && self.status != RecoveryProcessStatus::Failed
            && self.status != RecoveryProcessStatus::Completed
    }
}

/// Recovery state
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct RecoveryState {
    /// All guardians (guardians protecting our account)
    pub guardians: Vec<Guardian>,
    /// Current threshold (M of N)
    pub threshold: u32,
    /// Total guardian count
    pub guardian_count: u32,
    /// Active recovery process (if any)
    pub active_recovery: Option<RecoveryProcess>,
    /// Recovery requests for accounts we're a guardian of
    pub pending_requests: Vec<RecoveryProcess>,
    /// Accounts we are a guardian for (our guardian bindings)
    pub guardian_bindings: Vec<GuardianBinding>,
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
    pub fn guardian(&self, id: &AuthorityId) -> Option<&Guardian> {
        self.guardians.iter().find(|g| g.id == *id)
    }

    /// Initiate a recovery process
    pub fn initiate_recovery(
        &mut self,
        session_id: String,
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

    /// Add a guardian approval to the active recovery
    pub fn add_guardian_approval(&mut self, guardian_id: AuthorityId) {
        self.add_guardian_approval_with_timestamp(guardian_id, 0);
    }

    /// Add a guardian approval with timestamp to the active recovery
    pub fn add_guardian_approval_with_timestamp(
        &mut self,
        guardian_id: AuthorityId,
        timestamp: u64,
    ) {
        if let Some(ref mut recovery) = self.active_recovery {
            if !recovery.approved_by.contains(&guardian_id) {
                recovery.approved_by.push(guardian_id);
                recovery.approvals.push(RecoveryApproval {
                    guardian_id,
                    approved_at: timestamp,
                });
                recovery.approvals_received += 1;

                // Update progress
                if recovery.approvals_required > 0 {
                    recovery.progress =
                        (recovery.approvals_received * 100) / recovery.approvals_required;
                }

                // Update status based on approvals
                if recovery.approvals_received >= recovery.approvals_required {
                    recovery.status = RecoveryProcessStatus::Approved;
                    recovery.progress = 100;
                } else {
                    recovery.status = RecoveryProcessStatus::WaitingForApprovals;
                }
            }
        }
    }

    /// Complete the active recovery process
    pub fn complete_recovery(&mut self) {
        if let Some(ref mut recovery) = self.active_recovery {
            recovery.status = RecoveryProcessStatus::Completed;
            recovery.progress = 100;
        }
    }

    /// Fail/cancel the active recovery process
    pub fn fail_recovery(&mut self) {
        if let Some(ref mut recovery) = self.active_recovery {
            recovery.status = RecoveryProcessStatus::Failed;
        }
    }

    /// Clear the active recovery
    pub fn clear_recovery(&mut self) {
        self.active_recovery = None;
    }

    /// Toggle guardian status for a contact
    ///
    /// If is_guardian is true, adds/activates the guardian.
    /// If is_guardian is false, removes/revokes the guardian.
    pub fn toggle_guardian(&mut self, contact_id: AuthorityId, is_guardian: bool) {
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

    // === Guardian Binding Methods (accounts we are guardian for) ===

    /// Check if we are a guardian for a specific account.
    ///
    /// Returns true if we have an active guardian binding for the account.
    pub fn is_guardian_for(&self, account: &AuthorityId) -> bool {
        self.guardian_bindings
            .iter()
            .any(|binding| binding.account_authority == *account)
    }

    /// Add a guardian binding (we become guardian for an account).
    ///
    /// This records that we can approve recovery requests for the specified account.
    /// Duplicate bindings are prevented.
    pub fn add_guardian_for(
        &mut self,
        account: AuthorityId,
        context_id: ContextId,
        bound_at: u64,
    ) {
        // Prevent duplicate bindings
        if !self.is_guardian_for(&account) {
            self.guardian_bindings.push(GuardianBinding::new(
                account, context_id, bound_at,
            ));
        }
    }

    /// Add a guardian binding with account name.
    pub fn add_guardian_for_with_name(
        &mut self,
        account: AuthorityId,
        context_id: ContextId,
        bound_at: u64,
        account_name: impl Into<String>,
    ) {
        // Prevent duplicate bindings
        if !self.is_guardian_for(&account) {
            self.guardian_bindings.push(GuardianBinding::with_name(
                account,
                context_id,
                bound_at,
                account_name,
            ));
        }
    }

    /// Remove a guardian binding (we are no longer guardian for an account).
    pub fn remove_guardian_for(&mut self, account: &AuthorityId) {
        self.guardian_bindings
            .retain(|binding| binding.account_authority != *account);
    }

    /// Get a guardian binding by account.
    pub fn guardian_binding_for(&self, account: &AuthorityId) -> Option<&GuardianBinding> {
        self.guardian_bindings
            .iter()
            .find(|binding| binding.account_authority == *account)
    }

    /// Get the number of accounts we are guardian for.
    pub fn guardian_binding_count(&self) -> usize {
        self.guardian_bindings.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_recovery_process() -> RecoveryProcess {
        RecoveryProcess {
            id: "test-ceremony".to_string(),
            account_id: AuthorityId::default(),
            status: RecoveryProcessStatus::WaitingForApprovals,
            approvals_received: 0,
            approvals_required: 2,
            approved_by: vec![],
            approvals: vec![],
            initiated_at: 1000,
            expires_at: Some(2000),
            progress: 0,
        }
    }

    #[test]
    fn test_is_threshold_met_false() {
        let process = create_test_recovery_process();
        assert!(!process.is_threshold_met());
    }

    #[test]
    fn test_is_threshold_met_true() {
        let mut process = create_test_recovery_process();
        process.approvals_received = 2;
        assert!(process.is_threshold_met());
    }

    #[test]
    fn test_is_threshold_met_exceeds() {
        let mut process = create_test_recovery_process();
        process.approvals_received = 3; // More than required
        assert!(process.is_threshold_met());
    }

    #[test]
    fn test_progress_fraction_zero() {
        let process = create_test_recovery_process();
        assert_eq!(process.progress_fraction(), 0.0);
    }

    #[test]
    fn test_progress_fraction_half() {
        let mut process = create_test_recovery_process();
        process.approvals_received = 1;
        assert_eq!(process.progress_fraction(), 0.5);
    }

    #[test]
    fn test_progress_fraction_complete() {
        let mut process = create_test_recovery_process();
        process.approvals_received = 2;
        assert_eq!(process.progress_fraction(), 1.0);
    }

    #[test]
    fn test_progress_fraction_zero_threshold() {
        let mut process = create_test_recovery_process();
        process.approvals_required = 0;
        assert_eq!(process.progress_fraction(), 1.0);
    }

    #[test]
    fn test_has_guardian_approved_false() {
        let process = create_test_recovery_process();
        let guardian = AuthorityId::default();
        assert!(!process.has_guardian_approved(&guardian));
    }

    #[test]
    fn test_has_guardian_approved_true() {
        let mut process = create_test_recovery_process();
        let guardian = AuthorityId::default();
        process.approved_by.push(guardian);
        assert!(process.has_guardian_approved(&guardian));
    }

    #[test]
    fn test_can_complete_threshold_not_met() {
        let process = create_test_recovery_process();
        assert!(!process.can_complete());
    }

    #[test]
    fn test_can_complete_threshold_met() {
        let mut process = create_test_recovery_process();
        process.approvals_received = 2;
        assert!(process.can_complete());
    }

    #[test]
    fn test_can_complete_already_completed() {
        let mut process = create_test_recovery_process();
        process.approvals_received = 2;
        process.status = RecoveryProcessStatus::Completed;
        assert!(!process.can_complete());
    }

    #[test]
    fn test_can_complete_failed() {
        let mut process = create_test_recovery_process();
        process.approvals_received = 2;
        process.status = RecoveryProcessStatus::Failed;
        assert!(!process.can_complete());
    }

    // === Guardian Binding Tests ===

    fn test_authority_id(seed: u8) -> AuthorityId {
        let mut entropy = [0u8; 32];
        for (i, byte) in entropy.iter_mut().enumerate() {
            *byte = seed.wrapping_add(i as u8);
        }
        AuthorityId::new_from_entropy(entropy)
    }

    fn test_context_id(seed: u8) -> ContextId {
        let mut entropy = [0u8; 32];
        for (i, byte) in entropy.iter_mut().enumerate() {
            *byte = seed.wrapping_mul(2).wrapping_add(i as u8);
        }
        ContextId::new_from_entropy(entropy)
    }

    #[test]
    fn test_guardian_binding_new() {
        let account = test_authority_id(1);
        let context = test_context_id(1);
        let binding = GuardianBinding::new(account, context, 1000);

        assert_eq!(binding.account_authority, account);
        assert_eq!(binding.context_id, context);
        assert_eq!(binding.bound_at, 1000);
        assert!(binding.account_name.is_none());
    }

    #[test]
    fn test_guardian_binding_with_name() {
        let account = test_authority_id(1);
        let context = test_context_id(1);
        let binding = GuardianBinding::with_name(account, context, 1000, "Alice");

        assert_eq!(binding.account_authority, account);
        assert_eq!(binding.account_name, Some("Alice".to_string()));
    }

    #[test]
    fn test_is_guardian_for_false() {
        let state = RecoveryState::default();
        let account = test_authority_id(1);
        assert!(!state.is_guardian_for(&account));
    }

    #[test]
    fn test_is_guardian_for_true() {
        let mut state = RecoveryState::default();
        let account = test_authority_id(1);
        let context = test_context_id(1);
        state.add_guardian_for(account, context, 1000);
        assert!(state.is_guardian_for(&account));
    }

    #[test]
    fn test_add_guardian_for_prevents_duplicates() {
        let mut state = RecoveryState::default();
        let account = test_authority_id(1);
        let context = test_context_id(1);

        state.add_guardian_for(account, context, 1000);
        state.add_guardian_for(account, context, 2000); // Duplicate

        assert_eq!(state.guardian_binding_count(), 1);
        assert_eq!(state.guardian_bindings[0].bound_at, 1000); // First one preserved
    }

    #[test]
    fn test_remove_guardian_for() {
        let mut state = RecoveryState::default();
        let account1 = test_authority_id(1);
        let account2 = test_authority_id(2);
        let context = test_context_id(1);

        state.add_guardian_for(account1, context, 1000);
        state.add_guardian_for(account2, context, 1000);
        assert_eq!(state.guardian_binding_count(), 2);

        state.remove_guardian_for(&account1);
        assert_eq!(state.guardian_binding_count(), 1);
        assert!(!state.is_guardian_for(&account1));
        assert!(state.is_guardian_for(&account2));
    }

    #[test]
    fn test_guardian_binding_for() {
        let mut state = RecoveryState::default();
        let account = test_authority_id(1);
        let context = test_context_id(1);

        assert!(state.guardian_binding_for(&account).is_none());

        state.add_guardian_for(account, context, 1000);

        let binding = state.guardian_binding_for(&account);
        assert!(binding.is_some());
        assert_eq!(binding.unwrap().context_id, context);
    }
}
