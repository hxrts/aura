//! Authentication View Delta and Reducer
//!
//! This module provides view deltas and reducers for authentication facts.
//! Views are derived state computed from the append-only fact log.
//!
//! # Architecture
//!
//! Views follow the pattern established in `aura-journal`:
//! - Facts are immutable, append-only records
//! - Views are derived state computed from facts
//! - Reducers transform facts into view deltas
//! - Views are incrementally updated by applying deltas
//!
//! # View Types
//!
//! - `AuthView`: Aggregated authentication state
//! - `SessionView`: Active sessions for an authority
//! - `RecoveryView`: Pending recovery operations
//! - `GuardianApprovalView`: Guardian approval status

use crate::facts::{AuthFact, AuthFactDelta, AuthFactReducer};
use crate::guards::RecoveryOperationType;
use aura_core::identifiers::AuthorityId;
use aura_verify::session::SessionScope;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// =============================================================================
// Authentication View
// =============================================================================

/// Aggregated authentication view state
///
/// This view represents the current authentication state for an authority,
/// including active sessions, pending challenges, and recovery operations.
#[derive(Debug, Clone, Default)]
pub struct AuthView {
    /// Active sessions indexed by session ID
    pub active_sessions: HashMap<String, SessionInfo>,

    /// Pending challenges indexed by session ID
    pub pending_challenges: HashMap<String, ChallengeInfo>,

    /// Pending recovery operations indexed by request ID
    pub pending_recoveries: HashMap<String, RecoveryInfo>,

    /// Guardian approvals by request ID
    pub guardian_approvals: HashMap<String, Vec<AuthorityId>>,

    /// Recent authentication failures for rate limiting
    pub recent_failures: Vec<FailureRecord>,
}

/// Information about an active session
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionInfo {
    /// Session ID
    pub session_id: String,
    /// Authority the session belongs to
    pub authority_id: AuthorityId,
    /// Session scope
    pub scope: SessionScope,
    /// Issued timestamp (ms)
    pub issued_at_ms: u64,
    /// Expiration timestamp (ms)
    pub expires_at_ms: u64,
}

/// Information about a pending challenge
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChallengeInfo {
    /// Session ID for the challenge
    pub session_id: String,
    /// Authority requesting authentication
    pub authority_id: AuthorityId,
    /// Expiration timestamp (ms)
    pub expires_at_ms: u64,
}

/// Information about a pending recovery operation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryInfo {
    /// Request ID
    pub request_id: String,
    /// Account being recovered
    pub account_id: AuthorityId,
    /// Requester authority
    pub requester_id: AuthorityId,
    /// Recovery operation type
    pub operation_type: RecoveryOperationType,
    /// Required number of guardian approvals
    pub required_guardians: usize,
    /// Current approval count
    pub approval_count: usize,
    /// Guardians who have approved
    pub approvers: Vec<AuthorityId>,
    /// Whether this is an emergency operation
    pub is_emergency: bool,
    /// Expiration timestamp (ms)
    pub expires_at_ms: u64,
}

/// Record of an authentication failure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FailureRecord {
    /// Session ID that failed
    pub session_id: String,
    /// Authority that failed
    pub authority_id: AuthorityId,
    /// Failure reason
    pub reason: String,
    /// Failure timestamp (ms)
    pub failed_at_ms: u64,
}

impl AuthView {
    /// Create a new empty authentication view
    pub fn new() -> Self {
        Self::default()
    }

    /// Check if a session is active and not expired
    pub fn is_session_active(&self, session_id: &str, now_ms: u64) -> bool {
        self.active_sessions
            .get(session_id)
            .map(|s| s.expires_at_ms > now_ms)
            .unwrap_or(false)
    }

    /// Get all active sessions for an authority
    pub fn sessions_for_authority(&self, authority_id: AuthorityId) -> Vec<&SessionInfo> {
        self.active_sessions
            .values()
            .filter(|s| s.authority_id == authority_id)
            .collect()
    }

    /// Check if a recovery request has met its approval threshold
    pub fn is_recovery_approved(&self, request_id: &str) -> bool {
        self.pending_recoveries
            .get(request_id)
            .is_some_and(|r| r.approval_count >= r.required_guardians)
    }

    /// Get expired sessions that should be cleaned up
    pub fn get_expired_sessions(&self, now_ms: u64) -> Vec<String> {
        self.active_sessions
            .iter()
            .filter(|(_, s)| s.expires_at_ms <= now_ms)
            .map(|(id, _)| id.clone())
            .collect()
    }

    /// Get expired challenges that should be cleaned up
    pub fn get_expired_challenges(&self, now_ms: u64) -> Vec<String> {
        self.pending_challenges
            .iter()
            .filter(|(_, c)| c.expires_at_ms <= now_ms)
            .map(|(id, _)| id.clone())
            .collect()
    }

    /// Get expired recovery requests that should be cleaned up
    pub fn get_expired_recoveries(&self, now_ms: u64) -> Vec<String> {
        self.pending_recoveries
            .iter()
            .filter(|(_, r)| r.expires_at_ms <= now_ms)
            .map(|(id, _)| id.clone())
            .collect()
    }
}

// =============================================================================
// View Reducer
// =============================================================================

/// Reducer for authentication views
///
/// Transforms authentication facts into view state changes.
#[derive(Debug, Clone, Default)]
pub struct AuthViewReducer {
    fact_reducer: AuthFactReducer,
}

impl AuthViewReducer {
    /// Create a new view reducer
    pub fn new() -> Self {
        Self {
            fact_reducer: AuthFactReducer::new(),
        }
    }

    /// Apply a fact to the view and return the updated view
    pub fn apply(&self, view: &mut AuthView, fact: &AuthFact) {
        let delta = self.fact_reducer.reduce(fact);
        self.apply_delta(view, delta, fact);
    }

    /// Apply a delta to the view
    fn apply_delta(&self, view: &mut AuthView, delta: AuthFactDelta, fact: &AuthFact) {
        match delta {
            AuthFactDelta::NoChange => {}

            AuthFactDelta::PendingChallenge {
                session_id,
                authority_id,
                expires_at_ms,
            } => {
                view.pending_challenges.insert(
                    session_id.clone(),
                    ChallengeInfo {
                        session_id,
                        authority_id,
                        expires_at_ms,
                    },
                );
            }

            AuthFactDelta::ActiveSession {
                session_id,
                authority_id,
                scope,
                expires_at_ms,
            } => {
                // Remove from pending challenges
                view.pending_challenges.remove(&session_id);

                // Get issued_at from the fact
                let issued_at_ms = fact.timestamp_ms();

                // Add to active sessions
                view.active_sessions.insert(
                    session_id.clone(),
                    SessionInfo {
                        session_id,
                        authority_id,
                        scope,
                        issued_at_ms,
                        expires_at_ms,
                    },
                );
            }

            AuthFactDelta::SessionRemoved { session_id } => {
                view.active_sessions.remove(&session_id);
            }

            AuthFactDelta::PendingRecovery {
                request_id,
                account_id,
                required_guardians,
                expires_at_ms,
                ..
            } => {
                // Extract additional info from the fact
                let (requester_id, operation_type, is_emergency) =
                    if let AuthFact::GuardianApprovalRequested {
                        requester_id,
                        operation_type,
                        is_emergency,
                        ..
                    } = fact
                    {
                        (*requester_id, operation_type.clone(), *is_emergency)
                    } else {
                        (
                            account_id,
                            RecoveryOperationType::AccountAccessRecovery,
                            false,
                        )
                    };

                view.pending_recoveries.insert(
                    request_id.clone(),
                    RecoveryInfo {
                        request_id,
                        account_id,
                        requester_id,
                        operation_type,
                        required_guardians,
                        approval_count: 0,
                        approvers: vec![],
                        is_emergency,
                        expires_at_ms,
                    },
                );
            }

            AuthFactDelta::GuardianApprovalAdded {
                request_id,
                guardian_id,
            } => {
                // Update approval count in pending recovery
                if let Some(recovery) = view.pending_recoveries.get_mut(&request_id) {
                    if !recovery.approvers.contains(&guardian_id) {
                        recovery.approvers.push(guardian_id);
                        recovery.approval_count += 1;
                    }
                }

                // Also track in guardian_approvals map
                view.guardian_approvals
                    .entry(request_id)
                    .or_default()
                    .push(guardian_id);
            }

            AuthFactDelta::RecoveryCompleted { request_id } => {
                view.pending_recoveries.remove(&request_id);
            }

            AuthFactDelta::RecoveryFailed { request_id } => {
                view.pending_recoveries.remove(&request_id);
            }
        }

        // Handle failure records from AuthFailed facts
        if let AuthFact::AuthFailed {
            session_id,
            authority_id,
            reason,
            failed_at_ms,
        } = fact
        {
            view.recent_failures.push(FailureRecord {
                session_id: session_id.clone(),
                authority_id: *authority_id,
                reason: reason.clone(),
                failed_at_ms: *failed_at_ms,
            });

            // Keep only last 100 failures
            if view.recent_failures.len() > 100 {
                view.recent_failures.remove(0);
            }
        }
    }

    /// Reduce a sequence of facts into a view
    pub fn reduce_all(&self, facts: &[AuthFact]) -> AuthView {
        let mut view = AuthView::new();
        for fact in facts {
            self.apply(&mut view, fact);
        }
        view
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use aura_verify::session::SessionScope;

    fn test_authority() -> AuthorityId {
        AuthorityId::new_from_entropy([1u8; 32])
    }

    fn test_authority_2() -> AuthorityId {
        AuthorityId::new_from_entropy([2u8; 32])
    }

    fn test_context_id() -> ContextId {
        ContextId::new_from_entropy([9u8; 32])
    }

    #[test]
    fn test_auth_view_new() {
        let view = AuthView::new();
        assert!(view.active_sessions.is_empty());
        assert!(view.pending_challenges.is_empty());
        assert!(view.pending_recoveries.is_empty());
    }

    #[test]
    fn test_session_lifecycle() {
        let reducer = AuthViewReducer::new();
        let mut view = AuthView::new();

        // Issue a session
        let fact = AuthFact::SessionIssued {
            context_id: test_context_id(),
            session_id: "session_123".to_string(),
            authority_id: test_authority(),
            device_id: None,
            scope: SessionScope::Protocol {
                protocol_type: "test".to_string(),
            },
            issued_at_ms: 1000,
            expires_at_ms: 2000,
        };

        reducer.apply(&mut view, &fact);
        assert!(view.active_sessions.contains_key("session_123"));
        assert!(view.is_session_active("session_123", 1500));
        assert!(!view.is_session_active("session_123", 2500)); // Expired

        // Revoke the session
        let revoke_fact = AuthFact::SessionRevoked {
            context_id: test_context_id(),
            session_id: "session_123".to_string(),
            revoked_by: test_authority(),
            reason: "User requested".to_string(),
            revoked_at_ms: 1500,
        };

        reducer.apply(&mut view, &revoke_fact);
        assert!(!view.active_sessions.contains_key("session_123"));
    }

    #[test]
    fn test_recovery_approval_flow() {
        let reducer = AuthViewReducer::new();
        let mut view = AuthView::new();

        // Request guardian approval
        let request_fact = AuthFact::GuardianApprovalRequested {
            request_id: "recovery_123".to_string(),
            account_id: test_authority(),
            requester_id: test_authority(),
            operation_type: RecoveryOperationType::DeviceKeyRecovery,
            required_guardians: 2,
            is_emergency: false,
            justification: "Lost device".to_string(),
            requested_at_ms: 1000,
            expires_at_ms: 86400000,
        };

        reducer.apply(&mut view, &request_fact);
        assert!(view.pending_recoveries.contains_key("recovery_123"));
        assert!(!view.is_recovery_approved("recovery_123"));

        // First guardian approves
        let approve1 = AuthFact::GuardianApproved {
            context_id: test_context_id(),
            request_id: "recovery_123".to_string(),
            guardian_id: test_authority(),
            signature: vec![0u8; 64],
            justification: "Approved".to_string(),
            approved_at_ms: 2000,
        };

        reducer.apply(&mut view, &approve1);
        assert_eq!(
            view.pending_recoveries
                .get("recovery_123")
                .map(|r| r.approval_count),
            Some(1)
        );
        assert!(!view.is_recovery_approved("recovery_123"));

        // Second guardian approves
        let approve2 = AuthFact::GuardianApproved {
            request_id: "recovery_123".to_string(),
            guardian_id: test_authority_2(),
            signature: vec![0u8; 64],
            justification: "Approved".to_string(),
            approved_at_ms: 3000,
        };

        reducer.apply(&mut view, &approve2);
        assert_eq!(
            view.pending_recoveries
                .get("recovery_123")
                .map(|r| r.approval_count),
            Some(2)
        );
        assert!(view.is_recovery_approved("recovery_123"));
    }

    #[test]
    fn test_failure_tracking() {
        let reducer = AuthViewReducer::new();
        let mut view = AuthView::new();

        let fail_fact = AuthFact::AuthFailed {
            session_id: "session_456".to_string(),
            authority_id: test_authority(),
            reason: "Invalid signature".to_string(),
            failed_at_ms: 1000,
        };

        reducer.apply(&mut view, &fail_fact);
        assert_eq!(view.recent_failures.len(), 1);
        assert_eq!(view.recent_failures[0].session_id, "session_456");
    }

    #[test]
    fn test_expired_session_detection() {
        let view = AuthView {
            active_sessions: {
                let mut map = HashMap::new();
                map.insert(
                    "session_old".to_string(),
                    SessionInfo {
                        session_id: "session_old".to_string(),
                        authority_id: test_authority(),
                        scope: SessionScope::Protocol {
                            protocol_type: "test".to_string(),
                        },
                        issued_at_ms: 0,
                        expires_at_ms: 1000,
                    },
                );
                map.insert(
                    "session_new".to_string(),
                    SessionInfo {
                        session_id: "session_new".to_string(),
                        authority_id: test_authority(),
                        scope: SessionScope::Protocol {
                            protocol_type: "test".to_string(),
                        },
                        issued_at_ms: 500,
                        expires_at_ms: 2000,
                    },
                );
                map
            },
            ..Default::default()
        };

        let expired = view.get_expired_sessions(1500);
        assert_eq!(expired.len(), 1);
        assert!(expired.contains(&"session_old".to_string()));
    }

    #[test]
    fn test_sessions_for_authority() {
        let view = AuthView {
            active_sessions: {
                let mut map = HashMap::new();
                map.insert(
                    "session_1".to_string(),
                    SessionInfo {
                        session_id: "session_1".to_string(),
                        authority_id: test_authority(),
                        scope: SessionScope::Protocol {
                            protocol_type: "test".to_string(),
                        },
                        issued_at_ms: 0,
                        expires_at_ms: 2000,
                    },
                );
                map.insert(
                    "session_2".to_string(),
                    SessionInfo {
                        session_id: "session_2".to_string(),
                        authority_id: test_authority_2(),
                        scope: SessionScope::Protocol {
                            protocol_type: "test".to_string(),
                        },
                        issued_at_ms: 0,
                        expires_at_ms: 2000,
                    },
                );
                map
            },
            ..Default::default()
        };

        let sessions = view.sessions_for_authority(test_authority());
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].session_id, "session_1");
    }

    #[test]
    fn test_reduce_all() {
        let reducer = AuthViewReducer::new();

        let facts = vec![
            AuthFact::SessionIssued {
                context_id: test_context_id(),
                session_id: "session_1".to_string(),
                authority_id: test_authority(),
                device_id: None,
                scope: SessionScope::Protocol {
                    protocol_type: "test".to_string(),
                },
                issued_at_ms: 1000,
                expires_at_ms: 2000,
            },
            AuthFact::SessionIssued {
                context_id: test_context_id(),
                session_id: "session_2".to_string(),
                authority_id: test_authority_2(),
                device_id: None,
                scope: SessionScope::Protocol {
                    protocol_type: "test".to_string(),
                },
                issued_at_ms: 1500,
                expires_at_ms: 2500,
            },
        ];

        let view = reducer.reduce_all(&facts);
        assert_eq!(view.active_sessions.len(), 2);
    }

    #[test]
    fn test_reduce_all_commutes_for_disjoint_sessions() {
        let reducer = AuthViewReducer::new();
        let fact_a = AuthFact::SessionIssued {
            context_id: test_context_id(),
            session_id: "session_a".to_string(),
            authority_id: test_authority(),
            device_id: None,
            scope: SessionScope::Protocol {
                protocol_type: "test".to_string(),
            },
            issued_at_ms: 1000,
            expires_at_ms: 2000,
        };
        let fact_b = AuthFact::SessionIssued {
            context_id: test_context_id(),
            session_id: "session_b".to_string(),
            authority_id: test_authority_2(),
            device_id: None,
            scope: SessionScope::Protocol {
                protocol_type: "test".to_string(),
            },
            issued_at_ms: 1000,
            expires_at_ms: 2000,
        };

        let view1 = reducer.reduce_all(&[fact_a.clone(), fact_b.clone()]);
        let view2 = reducer.reduce_all(&[fact_b, fact_a]);

        assert_eq!(view1.active_sessions.len(), view2.active_sessions.len());
        assert!(view1.active_sessions.contains_key("session_a"));
        assert!(view1.active_sessions.contains_key("session_b"));
        assert!(view2.active_sessions.contains_key("session_a"));
        assert!(view2.active_sessions.contains_key("session_b"));
    }
}
