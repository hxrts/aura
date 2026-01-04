//! # Recovery View State

use aura_core::identifiers::{AuthorityId, ContextId};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ============================================================================
// Recovery Error Types
// ============================================================================

/// Error type for recovery state operations.
///
/// These errors replace silent no-ops, making failures explicit and debuggable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecoveryError {
    /// No active recovery process exists
    NoActiveRecovery,
    /// The specified recovery was not found
    RecoveryNotFound(String),
    /// The specified guardian was not found
    GuardianNotFound(AuthorityId),
    /// The guardian has already approved this recovery
    AlreadyApproved(AuthorityId),
    /// Recovery is not in a state that allows this operation
    InvalidState {
        /// The expected state description
        expected: &'static str,
        /// The actual current status
        actual: RecoveryProcessStatus,
    },
    /// Guardian already exists (for add operations)
    GuardianAlreadyExists(AuthorityId),
}

// ============================================================================
// Ceremony Progress Tracking
// ============================================================================

/// General ceremony progress tracking for threshold-based ceremonies.
///
/// This is a portable type for tracking progress of any threshold ceremony:
/// - Guardian setup ceremonies
/// - Key rotation ceremonies
/// - MFA device ceremonies
/// - Recovery ceremonies
///
/// The TUI's `KeyRotationCeremonyUiState` can convert to/from this type.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct CeremonyProgress {
    /// Number of participants who have accepted/approved
    pub accepted_count: u32,
    /// Total number of participants
    pub total_count: u32,
    /// Threshold required for completion (M of N)
    pub threshold: u32,
}

impl CeremonyProgress {
    /// Create a new ceremony progress tracker
    #[must_use]
    pub fn new(accepted_count: u32, total_count: u32, threshold: u32) -> Self {
        Self {
            accepted_count,
            total_count,
            threshold,
        }
    }

    /// Check if the threshold has been met
    #[must_use]
    pub fn is_threshold_met(&self) -> bool {
        self.accepted_count >= self.threshold
    }

    /// Get progress as a fraction (0.0 to 1.0) towards threshold
    ///
    /// Returns 1.0 if threshold is 0 (no approvals required).
    #[must_use]
    pub fn progress_fraction(&self) -> f64 {
        if self.threshold == 0 {
            return 1.0;
        }
        f64::from(self.accepted_count) / f64::from(self.threshold)
    }

    /// Get progress as a percentage (0-100)
    #[must_use]
    pub fn progress_percentage(&self) -> u32 {
        if self.threshold == 0 {
            return 100;
        }
        ((self.accepted_count * 100) / self.threshold).min(100)
    }

    /// Get the number of additional approvals needed
    #[must_use]
    pub fn approvals_needed(&self) -> u32 {
        self.threshold.saturating_sub(self.accepted_count)
    }

    /// Check if the ceremony can proceed (threshold met)
    #[must_use]
    pub fn can_complete(&self) -> bool {
        self.is_threshold_met()
    }

    /// Get a human-readable status string
    #[must_use]
    pub fn status_text(&self) -> String {
        if self.is_threshold_met() {
            format!("{}/{} (ready)", self.accepted_count, self.threshold)
        } else {
            format!(
                "{}/{} ({} more needed)",
                self.accepted_count,
                self.threshold,
                self.approvals_needed()
            )
        }
    }

    /// Record an additional acceptance
    pub fn record_acceptance(&mut self) {
        self.accepted_count = self.accepted_count.saturating_add(1);
    }
}

// ============================================================================
// Security Level Classification
// ============================================================================

/// Security level classification for guardian threshold configurations.
///
/// Helps users understand the security implications of their threshold choices.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
pub enum SecurityLevel {
    /// No guardians configured
    #[default]
    None,
    /// k=1: Any single guardian can recover (least secure)
    Low,
    /// k < majority: Less than half required
    Medium,
    /// k >= majority: More than half required
    High,
    /// k=n: All guardians required (most secure)
    Maximum,
}

impl SecurityLevel {
    /// Get a human-readable description of this security level.
    #[must_use]
    pub fn description(&self) -> &'static str {
        match self {
            Self::None => "No guardians configured yet",
            Self::Low => "Low security: Any single guardian can recover",
            Self::Medium => "Medium security: Less than majority required",
            Self::High => "High security: Majority required",
            Self::Maximum => "Maximum security: All guardians required",
        }
    }

    /// Get a short label for this security level.
    #[must_use]
    pub fn label(&self) -> &'static str {
        match self {
            Self::None => "None",
            Self::Low => "Low",
            Self::Medium => "Medium",
            Self::High => "High",
            Self::Maximum => "Maximum",
        }
    }

    /// Check if this security level is considered safe for production use.
    ///
    /// Low security (k=1) is generally not recommended for production.
    #[must_use]
    pub fn is_recommended(&self) -> bool {
        matches!(self, Self::Medium | Self::High | Self::Maximum)
    }
}

/// Classify the security level of a guardian threshold configuration.
///
/// # Arguments
/// * `threshold` - Required number of guardians (k)
/// * `guardian_count` - Total number of guardians (n)
///
/// # Returns
/// The appropriate security level classification.
///
/// # Examples
/// ```rust
/// use aura_app::views::recovery::{classify_threshold_security, SecurityLevel};
///
/// assert_eq!(classify_threshold_security(0, 0), SecurityLevel::None);
/// assert_eq!(classify_threshold_security(1, 3), SecurityLevel::Low);
/// assert_eq!(classify_threshold_security(2, 5), SecurityLevel::Medium);
/// assert_eq!(classify_threshold_security(3, 5), SecurityLevel::High);
/// assert_eq!(classify_threshold_security(5, 5), SecurityLevel::Maximum);
/// ```
#[must_use]
pub fn classify_threshold_security(threshold: u32, guardian_count: u32) -> SecurityLevel {
    if guardian_count == 0 {
        SecurityLevel::None
    } else if threshold == guardian_count {
        // Check k=n first (Maximum takes precedence over Low for 1-of-1)
        SecurityLevel::Maximum
    } else if threshold == 1 {
        SecurityLevel::Low
    } else {
        let majority = (guardian_count / 2) + 1;
        if threshold >= majority {
            SecurityLevel::High
        } else {
            SecurityLevel::Medium
        }
    }
}

/// Get a formatted security hint for a threshold configuration.
///
/// Convenience function that returns the description string directly.
#[must_use]
pub fn security_level_hint(threshold: u32, guardian_count: u32) -> String {
    classify_threshold_security(threshold, guardian_count)
        .description()
        .to_string()
}

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
#[derive(Debug, Clone, Serialize, Deserialize)]
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
    pub fn new(account_authority: AuthorityId, context_id: ContextId, bound_at: u64) -> Self {
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
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct RecoveryApproval {
    /// Guardian ID who approved
    pub guardian_id: AuthorityId,
    /// Timestamp when approved (ms since epoch)
    pub approved_at: u64,
}

/// Active recovery process (if any)
#[derive(Debug, Clone, Serialize, Deserialize)]
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
    /// All guardians (guardians protecting our account) - keyed by AuthorityId
    #[serde(default)]
    guardians: HashMap<AuthorityId, Guardian>,
    /// Current threshold (M of N)
    threshold: u32,
    /// Active recovery process (if any)
    active_recovery: Option<RecoveryProcess>,
    /// Recovery requests for accounts we're a guardian of
    pending_requests: Vec<RecoveryProcess>,
    /// Accounts we are a guardian for (our guardian bindings)
    guardian_bindings: Vec<GuardianBinding>,
}

impl RecoveryState {
    // =========================================================================
    // Constructors
    // =========================================================================

    /// Create a new RecoveryState from its parts.
    ///
    /// Used by query results and tests.
    #[must_use]
    pub fn from_parts(
        guardians: HashMap<AuthorityId, Guardian>,
        threshold: u32,
        active_recovery: Option<RecoveryProcess>,
        pending_requests: Vec<RecoveryProcess>,
        guardian_bindings: Vec<GuardianBinding>,
    ) -> Self {
        Self {
            guardians,
            threshold,
            active_recovery,
            pending_requests,
            guardian_bindings,
        }
    }

    // =========================================================================
    // Query Methods (Accessors)
    // =========================================================================

    /// Get the current threshold (M of N).
    #[must_use]
    pub fn threshold(&self) -> u32 {
        self.threshold
    }

    /// Get the total number of guardians.
    #[must_use]
    pub fn guardian_count(&self) -> usize {
        self.guardians.len()
    }

    /// Get the number of active guardians.
    #[must_use]
    pub fn active_guardian_count(&self) -> usize {
        self.guardians
            .values()
            .filter(|g| g.status == GuardianStatus::Active)
            .count()
    }

    /// Check if recovery is possible (enough active guardians).
    #[must_use]
    pub fn can_recover(&self) -> bool {
        self.active_guardian_count() as u32 >= self.threshold
    }

    /// Get guardian by ID.
    #[must_use]
    pub fn guardian(&self, id: &AuthorityId) -> Option<&Guardian> {
        self.guardians.get(id)
    }

    /// Get mutable guardian by ID.
    pub fn guardian_mut(&mut self, id: &AuthorityId) -> Option<&mut Guardian> {
        self.guardians.get_mut(id)
    }

    /// Get all guardians as an iterator.
    pub fn all_guardians(&self) -> impl Iterator<Item = &Guardian> {
        self.guardians.values()
    }

    /// Check if a guardian exists.
    #[must_use]
    pub fn has_guardian(&self, id: &AuthorityId) -> bool {
        self.guardians.contains_key(id)
    }

    /// Get the active recovery process (if any).
    #[must_use]
    pub fn active_recovery(&self) -> Option<&RecoveryProcess> {
        self.active_recovery.as_ref()
    }

    /// Get mutable access to the active recovery process.
    pub fn active_recovery_mut(&mut self) -> Option<&mut RecoveryProcess> {
        self.active_recovery.as_mut()
    }

    /// Get pending recovery requests (for accounts we're guardian of).
    pub fn pending_requests(&self) -> &[RecoveryProcess] {
        &self.pending_requests
    }

    /// Get mutable access to pending requests.
    pub fn pending_requests_mut(&mut self) -> &mut Vec<RecoveryProcess> {
        &mut self.pending_requests
    }

    // =========================================================================
    // Recovery Process Management
    // =========================================================================

    /// Initiate a recovery process.
    ///
    /// Returns error if a recovery is already active.
    pub fn initiate_recovery(
        &mut self,
        session_id: String,
        account_id: AuthorityId,
        initiated_at: u64,
    ) -> Result<(), RecoveryError> {
        if self.active_recovery.is_some() {
            return Err(RecoveryError::InvalidState {
                expected: "no active recovery",
                actual: self
                    .active_recovery
                    .as_ref()
                    .map(|r| r.status)
                    .unwrap_or_default(),
            });
        }

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
        Ok(())
    }

    /// Add a guardian approval to the active recovery.
    ///
    /// Returns error if:
    /// - No active recovery exists
    /// - Guardian has already approved
    pub fn add_approval(&mut self, guardian_id: AuthorityId) -> Result<(), RecoveryError> {
        self.add_approval_with_timestamp(guardian_id, 0)
    }

    /// Add a guardian approval with timestamp to the active recovery.
    ///
    /// Returns error if:
    /// - No active recovery exists
    /// - Guardian has already approved
    pub fn add_approval_with_timestamp(
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

        // Update progress
        if recovery.approvals_required > 0 {
            recovery.progress = (recovery.approvals_received * 100) / recovery.approvals_required;
        }

        // Update status based on approvals
        if recovery.approvals_received >= recovery.approvals_required {
            recovery.status = RecoveryProcessStatus::Approved;
            recovery.progress = 100;
        } else {
            recovery.status = RecoveryProcessStatus::WaitingForApprovals;
        }

        Ok(())
    }

    /// Complete the active recovery process.
    ///
    /// Returns error if no active recovery exists.
    pub fn complete_recovery(&mut self) -> Result<(), RecoveryError> {
        let recovery = self
            .active_recovery
            .as_mut()
            .ok_or(RecoveryError::NoActiveRecovery)?;

        recovery.status = RecoveryProcessStatus::Completed;
        recovery.progress = 100;
        Ok(())
    }

    /// Fail/cancel the active recovery process.
    ///
    /// Returns error if no active recovery exists.
    pub fn fail_recovery(&mut self) -> Result<(), RecoveryError> {
        let recovery = self
            .active_recovery
            .as_mut()
            .ok_or(RecoveryError::NoActiveRecovery)?;

        recovery.status = RecoveryProcessStatus::Failed;
        Ok(())
    }

    /// Clear the active recovery.
    pub fn clear_recovery(&mut self) {
        self.active_recovery = None;
    }

    // =========================================================================
    // Guardian Management
    // =========================================================================

    /// Add a new guardian.
    ///
    /// Returns error if guardian already exists.
    pub fn add_guardian(&mut self, guardian: Guardian) -> Result<(), RecoveryError> {
        if self.guardians.contains_key(&guardian.id) {
            return Err(RecoveryError::GuardianAlreadyExists(guardian.id));
        }
        self.guardians.insert(guardian.id, guardian);
        Ok(())
    }

    /// Apply a guardian (insert or replace).
    ///
    /// Unlike `add_guardian`, this always succeeds and replaces existing.
    pub fn apply_guardian(&mut self, guardian: Guardian) {
        self.guardians.insert(guardian.id, guardian);
    }

    /// Update an existing guardian.
    ///
    /// Returns error if guardian doesn't exist.
    pub fn update_guardian(
        &mut self,
        id: &AuthorityId,
        f: impl FnOnce(&mut Guardian),
    ) -> Result<(), RecoveryError> {
        let guardian = self
            .guardians
            .get_mut(id)
            .ok_or_else(|| RecoveryError::GuardianNotFound(*id))?;
        f(guardian);
        Ok(())
    }

    /// Revoke a guardian's status.
    ///
    /// Returns error if guardian doesn't exist.
    pub fn revoke_guardian(&mut self, id: &AuthorityId) -> Result<(), RecoveryError> {
        self.update_guardian(id, |g| g.status = GuardianStatus::Revoked)
    }

    /// Reactivate a revoked guardian.
    ///
    /// Returns error if guardian doesn't exist.
    pub fn reactivate_guardian(&mut self, id: &AuthorityId) -> Result<(), RecoveryError> {
        self.update_guardian(id, |g| g.status = GuardianStatus::Active)
    }

    /// Remove a guardian completely.
    ///
    /// Returns the removed guardian, or None if not found.
    pub fn remove_guardian(&mut self, id: &AuthorityId) -> Option<Guardian> {
        self.guardians.remove(id)
    }

    /// Set the recovery threshold.
    pub fn set_threshold(&mut self, threshold: u32) {
        self.threshold = threshold;
    }

    // =========================================================================
    // Legacy Compatibility Methods
    // =========================================================================

    /// Toggle guardian status for a contact (legacy method).
    ///
    /// **Deprecated**: Prefer `add_guardian()`, `revoke_guardian()`, or `reactivate_guardian()`.
    ///
    /// If is_guardian is true, adds/activates the guardian.
    /// If is_guardian is false, revokes the guardian.
    #[deprecated(note = "Use add_guardian/revoke_guardian/reactivate_guardian instead")]
    pub fn toggle_guardian(&mut self, contact_id: AuthorityId, is_guardian: bool) {
        if is_guardian {
            if self.guardians.contains_key(&contact_id) {
                // Reactivate existing guardian
                let _ = self.reactivate_guardian(&contact_id);
            } else {
                // Add new guardian
                let _ = self.add_guardian(Guardian {
                    id: contact_id,
                    name: String::new(),
                    status: GuardianStatus::Active,
                    added_at: 0,
                    last_seen: None,
                });
            }
        } else {
            // Revoke guardian status
            let _ = self.revoke_guardian(&contact_id);
        }
    }

    /// Add a guardian approval to the active recovery (legacy method).
    ///
    /// **Deprecated**: Prefer `add_approval()` which returns Result.
    ///
    /// Silently ignores if no active recovery or already approved.
    #[deprecated(note = "Use add_approval() which returns Result")]
    pub fn add_guardian_approval(&mut self, guardian_id: AuthorityId) {
        let _ = self.add_approval(guardian_id);
    }

    /// Add a guardian approval with timestamp (legacy method).
    ///
    /// **Deprecated**: Prefer `add_approval_with_timestamp()` which returns Result.
    #[deprecated(note = "Use add_approval_with_timestamp() which returns Result")]
    pub fn add_guardian_approval_with_timestamp(
        &mut self,
        guardian_id: AuthorityId,
        timestamp: u64,
    ) {
        let _ = self.add_approval_with_timestamp(guardian_id, timestamp);
    }

    // =========================================================================
    // Guardian Binding Methods (accounts we are guardian for)
    // =========================================================================

    /// Check if we are a guardian for a specific account.
    #[must_use]
    pub fn is_guardian_for(&self, account: &AuthorityId) -> bool {
        self.guardian_bindings
            .iter()
            .any(|binding| binding.account_authority == *account)
    }

    /// Add a guardian binding (we become guardian for an account).
    ///
    /// Duplicate bindings are prevented (silently ignored).
    pub fn add_guardian_for(&mut self, account: AuthorityId, context_id: ContextId, bound_at: u64) {
        if !self.is_guardian_for(&account) {
            self.guardian_bindings
                .push(GuardianBinding::new(account, context_id, bound_at));
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
    #[must_use]
    pub fn guardian_binding_for(&self, account: &AuthorityId) -> Option<&GuardianBinding> {
        self.guardian_bindings
            .iter()
            .find(|binding| binding.account_authority == *account)
    }

    /// Get the number of accounts we are guardian for.
    #[must_use]
    pub fn guardian_binding_count(&self) -> usize {
        self.guardian_bindings.len()
    }

    /// Get all guardian bindings.
    pub fn all_guardian_bindings(&self) -> &[GuardianBinding] {
        &self.guardian_bindings
    }
}

// ============================================================================
// Recovery Status Formatting
// ============================================================================

/// Format a recovery status report from journal fact keys.
///
/// Produces a human-readable summary of active and completed recovery sessions,
/// suitable for CLI output or logging.
///
/// # Arguments
/// * `active` - List of active recovery session identifiers
/// * `completed` - List of completed recovery session identifiers
///
/// # Returns
/// A formatted multi-line string summarizing recovery status.
///
/// # Example
/// ```rust
/// use aura_app::views::recovery::format_recovery_status;
///
/// let active = vec!["session-1".to_string(), "session-2".to_string()];
/// let completed = vec!["old-session".to_string()];
/// let report = format_recovery_status(&active, &completed);
///
/// assert!(report.contains("Found 2 active recovery session(s)"));
/// assert!(report.contains("session-1"));
/// assert!(report.contains("Completed recovery sessions (1)"));
/// ```
#[must_use]
pub fn format_recovery_status(active: &[String], completed: &[String]) -> String {
    use std::fmt::Write;

    let mut output = String::new();

    if active.is_empty() {
        let _ = writeln!(output, "No active recovery sessions found.");
    } else {
        let _ = writeln!(output, "Found {} active recovery session(s):", active.len());
        for (idx, key) in active.iter().enumerate() {
            let _ = writeln!(output, "  {}. {}", idx + 1, key);
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

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_recovery_process() -> RecoveryProcess {
        RecoveryProcess {
            id: "test-ceremony".to_string(),
            account_id: AuthorityId::new_from_entropy([1u8; 32]),
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
        let guardian = AuthorityId::new_from_entropy([1u8; 32]);
        assert!(!process.has_guardian_approved(&guardian));
    }

    #[test]
    fn test_has_guardian_approved_true() {
        let mut process = create_test_recovery_process();
        let guardian = AuthorityId::new_from_entropy([1u8; 32]);
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

    // === CeremonyProgress Tests ===

    #[test]
    fn test_ceremony_progress_new() {
        let progress = CeremonyProgress::new(1, 3, 2);
        assert_eq!(progress.accepted_count, 1);
        assert_eq!(progress.total_count, 3);
        assert_eq!(progress.threshold, 2);
    }

    #[test]
    fn test_ceremony_is_threshold_met_false() {
        let progress = CeremonyProgress::new(1, 3, 2);
        assert!(!progress.is_threshold_met());
    }

    #[test]
    fn test_ceremony_is_threshold_met_true() {
        let progress = CeremonyProgress::new(2, 3, 2);
        assert!(progress.is_threshold_met());
    }

    #[test]
    fn test_ceremony_is_threshold_met_exceeds() {
        let progress = CeremonyProgress::new(3, 3, 2);
        assert!(progress.is_threshold_met());
    }

    #[test]
    fn test_ceremony_progress_fraction_zero() {
        let progress = CeremonyProgress::new(0, 3, 2);
        assert_eq!(progress.progress_fraction(), 0.0);
    }

    #[test]
    fn test_ceremony_progress_fraction_half() {
        let progress = CeremonyProgress::new(1, 3, 2);
        assert_eq!(progress.progress_fraction(), 0.5);
    }

    #[test]
    fn test_ceremony_progress_fraction_complete() {
        let progress = CeremonyProgress::new(2, 3, 2);
        assert_eq!(progress.progress_fraction(), 1.0);
    }

    #[test]
    fn test_ceremony_progress_fraction_zero_threshold() {
        let progress = CeremonyProgress::new(0, 0, 0);
        assert_eq!(progress.progress_fraction(), 1.0);
    }

    #[test]
    fn test_ceremony_progress_percentage() {
        assert_eq!(CeremonyProgress::new(0, 3, 2).progress_percentage(), 0);
        assert_eq!(CeremonyProgress::new(1, 3, 2).progress_percentage(), 50);
        assert_eq!(CeremonyProgress::new(2, 3, 2).progress_percentage(), 100);
        assert_eq!(CeremonyProgress::new(3, 3, 2).progress_percentage(), 100); // capped
    }

    #[test]
    fn test_ceremony_approvals_needed() {
        assert_eq!(CeremonyProgress::new(0, 3, 2).approvals_needed(), 2);
        assert_eq!(CeremonyProgress::new(1, 3, 2).approvals_needed(), 1);
        assert_eq!(CeremonyProgress::new(2, 3, 2).approvals_needed(), 0);
        assert_eq!(CeremonyProgress::new(3, 3, 2).approvals_needed(), 0); // saturates
    }

    #[test]
    fn test_ceremony_can_complete() {
        assert!(!CeremonyProgress::new(1, 3, 2).can_complete());
        assert!(CeremonyProgress::new(2, 3, 2).can_complete());
    }

    #[test]
    fn test_ceremony_status_text() {
        assert_eq!(
            CeremonyProgress::new(1, 3, 2).status_text(),
            "1/2 (1 more needed)"
        );
        assert_eq!(CeremonyProgress::new(2, 3, 2).status_text(), "2/2 (ready)");
    }

    #[test]
    fn test_ceremony_record_acceptance() {
        let mut progress = CeremonyProgress::new(0, 3, 2);
        assert_eq!(progress.accepted_count, 0);

        progress.record_acceptance();
        assert_eq!(progress.accepted_count, 1);

        progress.record_acceptance();
        assert_eq!(progress.accepted_count, 2);
        assert!(progress.is_threshold_met());
    }

    // ========================================================================
    // Security Level Tests
    // ========================================================================

    #[test]
    fn test_security_level_none() {
        assert_eq!(classify_threshold_security(0, 0), SecurityLevel::None);
        assert_eq!(classify_threshold_security(1, 0), SecurityLevel::None);
    }

    #[test]
    fn test_security_level_low() {
        assert_eq!(classify_threshold_security(1, 3), SecurityLevel::Low);
        assert_eq!(classify_threshold_security(1, 5), SecurityLevel::Low);
        assert_eq!(classify_threshold_security(1, 1), SecurityLevel::Maximum); // k=n case
    }

    #[test]
    fn test_security_level_medium() {
        // Less than majority required
        assert_eq!(classify_threshold_security(2, 5), SecurityLevel::Medium);
        assert_eq!(classify_threshold_security(2, 6), SecurityLevel::Medium);
    }

    #[test]
    fn test_security_level_high() {
        // Majority or more required (but not all)
        assert_eq!(classify_threshold_security(3, 5), SecurityLevel::High);
        assert_eq!(classify_threshold_security(4, 5), SecurityLevel::High);
        assert_eq!(classify_threshold_security(2, 3), SecurityLevel::High);
    }

    #[test]
    fn test_security_level_maximum() {
        assert_eq!(classify_threshold_security(3, 3), SecurityLevel::Maximum);
        assert_eq!(classify_threshold_security(5, 5), SecurityLevel::Maximum);
    }

    #[test]
    fn test_security_level_description() {
        assert_eq!(
            SecurityLevel::None.description(),
            "No guardians configured yet"
        );
        assert_eq!(
            SecurityLevel::Low.description(),
            "Low security: Any single guardian can recover"
        );
        assert_eq!(
            SecurityLevel::Medium.description(),
            "Medium security: Less than majority required"
        );
        assert_eq!(
            SecurityLevel::High.description(),
            "High security: Majority required"
        );
        assert_eq!(
            SecurityLevel::Maximum.description(),
            "Maximum security: All guardians required"
        );
    }

    #[test]
    fn test_security_level_label() {
        assert_eq!(SecurityLevel::None.label(), "None");
        assert_eq!(SecurityLevel::Low.label(), "Low");
        assert_eq!(SecurityLevel::Medium.label(), "Medium");
        assert_eq!(SecurityLevel::High.label(), "High");
        assert_eq!(SecurityLevel::Maximum.label(), "Maximum");
    }

    #[test]
    fn test_security_level_is_recommended() {
        assert!(!SecurityLevel::None.is_recommended());
        assert!(!SecurityLevel::Low.is_recommended());
        assert!(SecurityLevel::Medium.is_recommended());
        assert!(SecurityLevel::High.is_recommended());
        assert!(SecurityLevel::Maximum.is_recommended());
    }

    #[test]
    fn test_security_level_hint() {
        assert_eq!(
            security_level_hint(2, 3),
            "High security: Majority required"
        );
        assert_eq!(
            security_level_hint(1, 3),
            "Low security: Any single guardian can recover"
        );
    }

    // === format_recovery_status Tests ===

    #[test]
    fn test_format_recovery_status_no_sessions() {
        let result = format_recovery_status(&[], &[]);
        assert!(result.contains("No active recovery sessions found."));
        assert!(!result.contains("Completed"));
    }

    #[test]
    fn test_format_recovery_status_active_only() {
        let active = vec!["session-1".to_string(), "session-2".to_string()];
        let result = format_recovery_status(&active, &[]);

        assert!(result.contains("Found 2 active recovery session(s):"));
        assert!(result.contains("1. session-1"));
        assert!(result.contains("2. session-2"));
        assert!(!result.contains("Completed"));
    }

    #[test]
    fn test_format_recovery_status_completed_only() {
        let completed = vec!["old-session".to_string()];
        let result = format_recovery_status(&[], &completed);

        assert!(result.contains("No active recovery sessions found."));
        assert!(result.contains("Completed recovery sessions (1):"));
        assert!(result.contains("- old-session"));
    }

    #[test]
    fn test_format_recovery_status_mixed() {
        let active = vec!["active-1".to_string()];
        let completed = vec!["done-1".to_string(), "done-2".to_string()];
        let result = format_recovery_status(&active, &completed);

        assert!(result.contains("Found 1 active recovery session(s):"));
        assert!(result.contains("1. active-1"));
        assert!(result.contains("Completed recovery sessions (2):"));
        assert!(result.contains("- done-1"));
        assert!(result.contains("- done-2"));
    }
}
