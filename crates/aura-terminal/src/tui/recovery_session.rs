//! # Recovery Session State
//!
//! State management for active recovery ceremonies in the TUI.
//!
//! Recovery is a multi-step process requiring state persistence across multiple commands:
//! 1. StartRecovery - Creates session with RecoveryProtocol
//! 2. SubmitGuardianApproval - Adds approvals to active session
//! 3. CompleteRecovery - Checks threshold and finalizes
//! 4. CancelRecovery - Cleans up session state

use aura_core::{AuthorityId, ContextId};

/// Active recovery session
///
/// Tracks state for a recovery ceremony across multiple TUI commands.
/// Only one recovery session can be active at a time per user.
#[derive(Clone)]
pub struct RecoverySession {
    /// Unique session identifier
    pub session_id: String,

    /// Account authority being recovered
    pub account_authority: AuthorityId,

    /// Guardian authorities (k-of-n)
    pub guardian_authorities: Vec<AuthorityId>,

    /// Threshold (minimum approvals required)
    pub threshold: usize,

    /// Relational context for recovery state storage
    pub recovery_context: ContextId,

    /// Collected guardian approvals
    pub approvals: Vec<GuardianApproval>,

    /// Session start timestamp (milliseconds since epoch)
    pub started_at: u64,

    /// Session status
    pub status: RecoveryStatus,
}

/// Guardian approval for recovery
#[derive(Clone, Debug)]
pub struct GuardianApproval {
    /// Guardian who provided approval
    pub guardian_id: AuthorityId,

    /// Recovery session this approval is for
    pub recovery_id: String,

    /// Approval signature (proves guardian authorization)
    pub signature: Vec<u8>,

    /// Timestamp of approval
    pub timestamp: u64,
}

/// Recovery session status
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RecoveryStatus {
    /// Session initiated, waiting for approvals
    Pending,

    /// Collecting guardian approvals
    CollectingApprovals {
        /// Number of approvals received so far
        received: usize,
        /// Total threshold required
        required: usize,
    },

    /// Threshold met, ready to finalize
    ThresholdMet,

    /// Recovery completed successfully
    Completed,

    /// Recovery cancelled by user
    Cancelled,

    /// Recovery failed with error
    Failed(String),
}

impl RecoverySession {
    /// Create a new recovery session
    pub fn new(
        session_id: String,
        account_authority: AuthorityId,
        guardian_authorities: Vec<AuthorityId>,
        threshold: usize,
        recovery_context: ContextId,
        started_at: u64,
    ) -> Self {
        Self {
            session_id,
            account_authority,
            guardian_authorities,
            threshold,
            recovery_context,
            approvals: Vec::new(),
            started_at,
            status: RecoveryStatus::Pending,
        }
    }

    /// Add a guardian approval to the session
    pub fn add_approval(&mut self, approval: GuardianApproval) -> Result<(), String> {
        // Validate guardian is in authorized list
        if !self.guardian_authorities.contains(&approval.guardian_id) {
            return Err(format!(
                "Guardian {} not authorized for this recovery",
                approval.guardian_id
            ));
        }

        // Check for duplicate approval
        if self
            .approvals
            .iter()
            .any(|a| a.guardian_id == approval.guardian_id)
        {
            return Err(format!(
                "Guardian {} already approved this recovery",
                approval.guardian_id
            ));
        }

        // Add approval
        self.approvals.push(approval);

        // Update status
        self.update_status();

        Ok(())
    }

    /// Check if threshold is met
    pub fn is_threshold_met(&self) -> bool {
        self.approvals.len() >= self.threshold
    }

    /// Update session status based on current approvals
    fn update_status(&mut self) {
        let received = self.approvals.len();
        let required = self.threshold;

        self.status = if self.is_threshold_met() {
            RecoveryStatus::ThresholdMet
        } else {
            RecoveryStatus::CollectingApprovals { received, required }
        };
    }

    /// Mark session as completed
    pub fn mark_completed(&mut self) {
        self.status = RecoveryStatus::Completed;
    }

    /// Mark session as cancelled
    pub fn mark_cancelled(&mut self) {
        self.status = RecoveryStatus::Cancelled;
    }

    /// Mark session as failed
    pub fn mark_failed(&mut self, error: String) {
        self.status = RecoveryStatus::Failed(error);
    }

    /// Get current approval count
    pub fn approval_count(&self) -> usize {
        self.approvals.len()
    }

    /// Get session progress as a fraction (0.0 to 1.0)
    pub fn progress(&self) -> f64 {
        if self.threshold == 0 {
            return 1.0;
        }
        (self.approvals.len() as f64) / (self.threshold as f64)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::ids;

    fn create_test_session() -> RecoverySession {
        let account = ids::authority_id("test:account");
        let guardian1 = ids::authority_id("test:guardian1");
        let guardian2 = ids::authority_id("test:guardian2");
        let guardian3 = ids::authority_id("test:guardian3");
        let guardians = vec![guardian1, guardian2, guardian3];

        let context = Arc::new(RelationalContext::new(guardians.clone()));

        RecoverySession::new(
            "session-123".to_string(),
            account,
            guardians,
            2, // 2-of-3 threshold
            context,
            1234567890,
        )
    }

    fn create_approval(guardian_id: AuthorityId, recovery_id: &str) -> GuardianApproval {
        GuardianApproval {
            guardian_id,
            recovery_id: recovery_id.to_string(),
            signature: vec![1, 2, 3], // Mock signature
            timestamp: 1234567890,
        }
    }

    #[test]
    fn test_new_session() {
        let session = create_test_session();

        assert_eq!(session.session_id, "session-123");
        assert_eq!(session.threshold, 2);
        assert_eq!(session.guardian_authorities.len(), 3);
        assert_eq!(session.approval_count(), 0);
        assert_eq!(session.status, RecoveryStatus::Pending);
        assert!(!session.is_threshold_met());
    }

    #[test]
    fn test_add_approval() {
        let mut session = create_test_session();
        let guardian1 = ids::authority_id("test:guardian1");

        let approval = create_approval(guardian1, "session-123");
        let result = session.add_approval(approval);

        assert!(result.is_ok());
        assert_eq!(session.approval_count(), 1);
        assert!(!session.is_threshold_met());
        assert_eq!(
            session.status,
            RecoveryStatus::CollectingApprovals {
                received: 1,
                required: 2
            }
        );
    }

    #[test]
    fn test_threshold_met() {
        let mut session = create_test_session();
        let guardian1 = ids::authority_id("test:guardian1");
        let guardian2 = ids::authority_id("test:guardian2");

        // Add first approval
        session
            .add_approval(create_approval(guardian1, "session-123"))
            .unwrap();
        assert!(!session.is_threshold_met());

        // Add second approval (meets threshold)
        session
            .add_approval(create_approval(guardian2, "session-123"))
            .unwrap();
        assert!(session.is_threshold_met());
        assert_eq!(session.status, RecoveryStatus::ThresholdMet);
    }

    #[test]
    fn test_duplicate_approval() {
        let mut session = create_test_session();
        let guardian1 = ids::authority_id("test:guardian1");

        let approval1 = create_approval(guardian1, "session-123");
        let approval2 = create_approval(guardian1, "session-123");

        // First approval succeeds
        assert!(session.add_approval(approval1).is_ok());

        // Second approval from same guardian fails
        let result = session.add_approval(approval2);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("already approved"));
    }

    #[test]
    fn test_unauthorized_guardian() {
        let mut session = create_test_session();
        let unauthorized = ids::authority_id("test:unauthorized");

        let approval = create_approval(unauthorized, "session-123");
        let result = session.add_approval(approval);

        assert!(result.is_err());
        assert!(result.unwrap_err().contains("not authorized"));
    }

    #[test]
    fn test_progress_calculation() {
        let mut session = create_test_session();
        assert_eq!(session.progress(), 0.0);

        let guardian1 = ids::authority_id("test:guardian1");
        session
            .add_approval(create_approval(guardian1, "session-123"))
            .unwrap();
        assert_eq!(session.progress(), 0.5); // 1/2

        let guardian2 = ids::authority_id("test:guardian2");
        session
            .add_approval(create_approval(guardian2, "session-123"))
            .unwrap();
        assert_eq!(session.progress(), 1.0); // 2/2
    }

    #[test]
    fn test_session_lifecycle() {
        let mut session = create_test_session();

        // Start: Pending
        assert_eq!(session.status, RecoveryStatus::Pending);

        // Add approvals: CollectingApprovals
        let guardian1 = ids::authority_id("test:guardian1");
        session
            .add_approval(create_approval(guardian1, "session-123"))
            .unwrap();
        assert!(matches!(
            session.status,
            RecoveryStatus::CollectingApprovals { .. }
        ));

        // Meet threshold: ThresholdMet
        let guardian2 = ids::authority_id("test:guardian2");
        session
            .add_approval(create_approval(guardian2, "session-123"))
            .unwrap();
        assert_eq!(session.status, RecoveryStatus::ThresholdMet);

        // Complete: Completed
        session.mark_completed();
        assert_eq!(session.status, RecoveryStatus::Completed);
    }

    #[test]
    fn test_cancel_session() {
        let mut session = create_test_session();

        session.mark_cancelled();
        assert_eq!(session.status, RecoveryStatus::Cancelled);
    }

    #[test]
    fn test_fail_session() {
        let mut session = create_test_session();

        session.mark_failed("Test error".to_string());
        assert_eq!(
            session.status,
            RecoveryStatus::Failed("Test error".to_string())
        );
    }
}
