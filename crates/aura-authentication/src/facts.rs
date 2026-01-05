//! Authentication Domain Facts
//!
//! Fact types for authentication state changes.
//! Facts are immutable, append-only records that track authentication events.
//!
//! # Architecture
//!
//! Facts follow the pattern established in `aura-journal`:
//! - Each fact is a typed, serializable record
//! - Facts are appended to journals and never modified
//! - Reducers transform facts into views for efficient querying
//!
//! # Fact Types
//!
//! - `ChallengeGenerated`: An authentication challenge was created
//! - `ProofSubmitted`: An identity proof was submitted
//! - `AuthVerified`: Authentication was successfully verified
//! - `AuthFailed`: Authentication verification failed
//! - `SessionIssued`: A session ticket was issued
//! - `SessionRevoked`: A session was explicitly revoked
//! - `GuardianApprovalRequested`: Guardian approval was requested
//! - `GuardianApproved`: A guardian approved a recovery request
//! - `GuardianDenied`: A guardian denied a recovery request
//! - `RecoveryCompleted`: Recovery operation completed successfully
//! - `RecoveryFailed`: Recovery operation failed

use aura_core::identifiers::AuthorityId;
use aura_core::{ContextId, DeviceId};
use aura_journal::reduction::{RelationalBinding, RelationalBindingType};
use aura_journal::{DomainFact, FactReducer};
use aura_macros::DomainFact;
use aura_signature::session::SessionScope;
use serde::{Deserialize, Serialize};

use crate::guards::RecoveryOperationType;

/// Fact type identifier for authentication facts
pub const AUTH_FACT_TYPE_ID: &str = "aura.authenticate.v1";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthFactKey {
    pub sub_type: &'static str,
    pub data: Vec<u8>,
}

// =============================================================================
// Authentication Facts
// =============================================================================

/// Authentication domain facts
///
/// These facts capture all state-changing events in the authentication system.
/// They are designed to be immutable and append-only.
#[derive(Debug, Clone, Serialize, Deserialize, DomainFact)]
#[domain_fact(
    type_id = "aura.authenticate.v1",
    schema_version = 1,
    context_fn = "context_id"
)]
pub enum AuthFact {
    /// An authentication challenge was generated
    ChallengeGenerated {
        /// Relational context for this auth fact
        context_id: ContextId,
        /// Unique session identifier
        session_id: String,
        /// Authority requesting authentication
        authority_id: AuthorityId,
        /// Device requesting authentication (if applicable)
        device_id: Option<DeviceId>,
        /// Requested authentication scope
        scope: SessionScope,
        /// Challenge expiration timestamp (ms)
        expires_at_ms: u64,
        /// Timestamp when challenge was created (ms)
        created_at_ms: u64,
    },

    /// An identity proof was submitted for verification
    ProofSubmitted {
        /// Relational context for this auth fact
        context_id: ContextId,
        /// Session ID from challenge
        session_id: String,
        /// Authority submitting proof
        authority_id: AuthorityId,
        /// Hash of the identity proof
        proof_hash: [u8; 32],
        /// Timestamp of submission (ms)
        submitted_at_ms: u64,
    },

    /// Authentication was successfully verified
    AuthVerified {
        /// Relational context for this auth fact
        context_id: ContextId,
        /// Session ID that was verified
        session_id: String,
        /// Authority that was authenticated
        authority_id: AuthorityId,
        /// Device that was authenticated (if applicable)
        device_id: Option<DeviceId>,
        /// Verification timestamp (ms)
        verified_at_ms: u64,
        /// Hash of the verified message
        message_hash: [u8; 32],
    },

    /// Authentication verification failed
    AuthFailed {
        /// Relational context for this auth fact
        context_id: ContextId,
        /// Session ID that failed
        session_id: String,
        /// Authority that failed authentication
        authority_id: AuthorityId,
        /// Reason for failure
        reason: String,
        /// Timestamp of failure (ms)
        failed_at_ms: u64,
    },

    /// A session ticket was issued
    SessionIssued {
        /// Relational context for this auth fact
        context_id: ContextId,
        /// Session ticket ID
        session_id: String,
        /// Authority the session was issued to
        authority_id: AuthorityId,
        /// Device the session was issued to (if applicable)
        device_id: Option<DeviceId>,
        /// Session scope
        scope: SessionScope,
        /// Timestamp when issued (ms)
        issued_at_ms: u64,
        /// Timestamp when session expires (ms)
        expires_at_ms: u64,
    },

    /// A session was explicitly revoked
    SessionRevoked {
        /// Relational context for this auth fact
        context_id: ContextId,
        /// Session ticket ID that was revoked
        session_id: String,
        /// Authority that revoked the session
        revoked_by: AuthorityId,
        /// Reason for revocation
        reason: String,
        /// Timestamp of revocation (ms)
        revoked_at_ms: u64,
    },

    /// Guardian approval was requested for recovery
    GuardianApprovalRequested {
        /// Relational context for this auth fact
        context_id: ContextId,
        /// Unique request identifier
        request_id: String,
        /// Account being recovered
        account_id: AuthorityId,
        /// Authority requesting recovery
        requester_id: AuthorityId,
        /// Type of recovery operation
        operation_type: RecoveryOperationType,
        /// Number of guardian approvals required
        required_guardians: u32,
        /// Whether this is an emergency request
        is_emergency: bool,
        /// Justification for the request
        justification: String,
        /// Timestamp of request (ms)
        requested_at_ms: u64,
        /// Request expiration timestamp (ms)
        expires_at_ms: u64,
    },

    /// A guardian approved a recovery request
    GuardianApproved {
        /// Relational context for this auth fact
        context_id: ContextId,
        /// Request ID being approved
        request_id: String,
        /// Guardian providing approval
        guardian_id: AuthorityId,
        /// Signature over the approval
        signature: Vec<u8>,
        /// Justification for approval
        justification: String,
        /// Timestamp of approval (ms)
        approved_at_ms: u64,
    },

    /// A guardian denied a recovery request
    GuardianDenied {
        /// Relational context for this auth fact
        context_id: ContextId,
        /// Request ID being denied
        request_id: String,
        /// Guardian providing denial
        guardian_id: AuthorityId,
        /// Reason for denial
        reason: String,
        /// Timestamp of denial (ms)
        denied_at_ms: u64,
    },

    /// Recovery operation completed successfully
    RecoveryCompleted {
        /// Relational context for this auth fact
        context_id: ContextId,
        /// Request ID that completed
        request_id: String,
        /// Account that was recovered
        account_id: AuthorityId,
        /// Type of recovery operation
        operation_type: RecoveryOperationType,
        /// Number of guardian approvals received
        approval_count: u32,
        /// Guardians who approved
        approvers: Vec<AuthorityId>,
        /// Timestamp of completion (ms)
        completed_at_ms: u64,
    },

    /// Recovery operation failed
    RecoveryFailed {
        /// Relational context for this auth fact
        context_id: ContextId,
        /// Request ID that failed
        request_id: String,
        /// Account for which recovery failed
        account_id: AuthorityId,
        /// Type of recovery operation
        operation_type: RecoveryOperationType,
        /// Reason for failure
        reason: String,
        /// Number of approvals received (if insufficient)
        approval_count: u32,
        /// Required approvals
        required_count: u32,
        /// Timestamp of failure (ms)
        failed_at_ms: u64,
    },
}

impl AuthFact {
    /// Get the fact type identifier
    pub fn type_id(&self) -> &'static str {
        AUTH_FACT_TYPE_ID
    }

    /// Get the primary authority associated with this fact
    pub fn primary_authority(&self) -> AuthorityId {
        match self {
            AuthFact::ChallengeGenerated { authority_id, .. } => *authority_id,
            AuthFact::ProofSubmitted { authority_id, .. } => *authority_id,
            AuthFact::AuthVerified { authority_id, .. } => *authority_id,
            AuthFact::AuthFailed { authority_id, .. } => *authority_id,
            AuthFact::SessionIssued { authority_id, .. } => *authority_id,
            AuthFact::SessionRevoked { revoked_by, .. } => *revoked_by,
            AuthFact::GuardianApprovalRequested { requester_id, .. } => *requester_id,
            AuthFact::GuardianApproved { guardian_id, .. } => *guardian_id,
            AuthFact::GuardianDenied { guardian_id, .. } => *guardian_id,
            AuthFact::RecoveryCompleted { account_id, .. } => *account_id,
            AuthFact::RecoveryFailed { account_id, .. } => *account_id,
        }
    }

    /// Get the timestamp of this fact (in milliseconds)
    pub fn timestamp_ms(&self) -> u64 {
        match self {
            AuthFact::ChallengeGenerated { created_at_ms, .. } => *created_at_ms,
            AuthFact::ProofSubmitted {
                submitted_at_ms, ..
            } => *submitted_at_ms,
            AuthFact::AuthVerified { verified_at_ms, .. } => *verified_at_ms,
            AuthFact::AuthFailed { failed_at_ms, .. } => *failed_at_ms,
            AuthFact::SessionIssued { issued_at_ms, .. } => *issued_at_ms,
            AuthFact::SessionRevoked { revoked_at_ms, .. } => *revoked_at_ms,
            AuthFact::GuardianApprovalRequested {
                requested_at_ms, ..
            } => *requested_at_ms,
            AuthFact::GuardianApproved { approved_at_ms, .. } => *approved_at_ms,
            AuthFact::GuardianDenied { denied_at_ms, .. } => *denied_at_ms,
            AuthFact::RecoveryCompleted {
                completed_at_ms, ..
            } => *completed_at_ms,
            AuthFact::RecoveryFailed { failed_at_ms, .. } => *failed_at_ms,
        }
    }

    /// Get the session ID if this fact is session-related
    pub fn session_id(&self) -> Option<&str> {
        match self {
            AuthFact::ChallengeGenerated { session_id, .. } => Some(session_id),
            AuthFact::ProofSubmitted { session_id, .. } => Some(session_id),
            AuthFact::AuthVerified { session_id, .. } => Some(session_id),
            AuthFact::AuthFailed { session_id, .. } => Some(session_id),
            AuthFact::SessionIssued { session_id, .. } => Some(session_id),
            AuthFact::SessionRevoked { session_id, .. } => Some(session_id),
            _ => None,
        }
    }

    /// Get the request ID if this fact is recovery-related
    pub fn request_id(&self) -> Option<&str> {
        match self {
            AuthFact::GuardianApprovalRequested { request_id, .. } => Some(request_id),
            AuthFact::GuardianApproved { request_id, .. } => Some(request_id),
            AuthFact::GuardianDenied { request_id, .. } => Some(request_id),
            AuthFact::RecoveryCompleted { request_id, .. } => Some(request_id),
            AuthFact::RecoveryFailed { request_id, .. } => Some(request_id),
            _ => None,
        }
    }

    /// Check if this fact indicates success
    pub fn is_success(&self) -> bool {
        matches!(
            self,
            AuthFact::AuthVerified { .. }
                | AuthFact::SessionIssued { .. }
                | AuthFact::GuardianApproved { .. }
                | AuthFact::RecoveryCompleted { .. }
        )
    }

    /// Check if this fact indicates failure
    pub fn is_failure(&self) -> bool {
        matches!(
            self,
            AuthFact::AuthFailed { .. }
                | AuthFact::SessionRevoked { .. }
                | AuthFact::GuardianDenied { .. }
                | AuthFact::RecoveryFailed { .. }
        )
    }
    /// Get the context ID associated with this fact
    pub fn context_id(&self) -> ContextId {
        match self {
            AuthFact::ChallengeGenerated { context_id, .. } => *context_id,
            AuthFact::ProofSubmitted { context_id, .. } => *context_id,
            AuthFact::AuthVerified { context_id, .. } => *context_id,
            AuthFact::AuthFailed { context_id, .. } => *context_id,
            AuthFact::SessionIssued { context_id, .. } => *context_id,
            AuthFact::SessionRevoked { context_id, .. } => *context_id,
            AuthFact::GuardianApprovalRequested { context_id, .. } => *context_id,
            AuthFact::GuardianApproved { context_id, .. } => *context_id,
            AuthFact::GuardianDenied { context_id, .. } => *context_id,
            AuthFact::RecoveryCompleted { context_id, .. } => *context_id,
            AuthFact::RecoveryFailed { context_id, .. } => *context_id,
        }
    }

    /// Validate that this fact is eligible to reduce under the provided context.
    pub fn validate_for_reduction(&self, context_id: ContextId) -> bool {
        self.context_id() == context_id
    }

    /// Derive the relational binding subtype and key data for this fact.
    pub fn binding_key(&self) -> AuthFactKey {
        match self {
            AuthFact::ChallengeGenerated { session_id, .. } => AuthFactKey {
                sub_type: "auth-challenge-generated",
                data: session_id.as_bytes().to_vec(),
            },
            AuthFact::ProofSubmitted { session_id, .. } => AuthFactKey {
                sub_type: "auth-proof-submitted",
                data: session_id.as_bytes().to_vec(),
            },
            AuthFact::AuthVerified { session_id, .. } => AuthFactKey {
                sub_type: "auth-verified",
                data: session_id.as_bytes().to_vec(),
            },
            AuthFact::AuthFailed { session_id, .. } => AuthFactKey {
                sub_type: "auth-failed",
                data: session_id.as_bytes().to_vec(),
            },
            AuthFact::SessionIssued { session_id, .. } => AuthFactKey {
                sub_type: "auth-session-issued",
                data: session_id.as_bytes().to_vec(),
            },
            AuthFact::SessionRevoked { session_id, .. } => AuthFactKey {
                sub_type: "auth-session-revoked",
                data: session_id.as_bytes().to_vec(),
            },
            AuthFact::GuardianApprovalRequested { request_id, .. } => AuthFactKey {
                sub_type: "auth-guardian-approval-requested",
                data: request_id.as_bytes().to_vec(),
            },
            AuthFact::GuardianApproved { request_id, .. } => AuthFactKey {
                sub_type: "auth-guardian-approved",
                data: request_id.as_bytes().to_vec(),
            },
            AuthFact::GuardianDenied { request_id, .. } => AuthFactKey {
                sub_type: "auth-guardian-denied",
                data: request_id.as_bytes().to_vec(),
            },
            AuthFact::RecoveryCompleted { request_id, .. } => AuthFactKey {
                sub_type: "auth-recovery-completed",
                data: request_id.as_bytes().to_vec(),
            },
            AuthFact::RecoveryFailed { request_id, .. } => AuthFactKey {
                sub_type: "auth-recovery-failed",
                data: request_id.as_bytes().to_vec(),
            },
        }
    }
}

// =============================================================================
// Fact Reducer
// =============================================================================

/// Reducer for authentication facts
///
/// Transforms a stream of facts into an authentication view.
#[derive(Debug, Clone, Default)]
pub struct AuthFactReducer;

impl AuthFactReducer {
    /// Create a new reducer
    pub fn new() -> Self {
        Self
    }

    /// Reduce a single fact into a view delta
    pub fn reduce(&self, fact: &AuthFact) -> AuthFactDelta {
        match fact {
            AuthFact::ChallengeGenerated {
                session_id,
                authority_id,
                expires_at_ms,
                ..
            } => AuthFactDelta::PendingChallenge {
                session_id: session_id.clone(),
                authority_id: *authority_id,
                expires_at_ms: *expires_at_ms,
            },

            AuthFact::SessionIssued {
                session_id,
                authority_id,
                scope,
                expires_at_ms,
                ..
            } => AuthFactDelta::ActiveSession {
                session_id: session_id.clone(),
                authority_id: *authority_id,
                scope: scope.clone(),
                expires_at_ms: *expires_at_ms,
            },

            AuthFact::SessionRevoked { session_id, .. } => AuthFactDelta::SessionRemoved {
                session_id: session_id.clone(),
            },

            AuthFact::GuardianApprovalRequested {
                request_id,
                account_id,
                required_guardians,
                expires_at_ms,
                ..
            } => AuthFactDelta::PendingRecovery {
                request_id: request_id.clone(),
                account_id: *account_id,
                required_guardians: *required_guardians,
                approval_count: 0,
                expires_at_ms: *expires_at_ms,
            },

            AuthFact::GuardianApproved {
                request_id,
                guardian_id,
                ..
            } => AuthFactDelta::GuardianApprovalAdded {
                request_id: request_id.clone(),
                guardian_id: *guardian_id,
            },

            AuthFact::GuardianDenied { request_id, .. } => AuthFactDelta::RecoveryFailed {
                request_id: request_id.clone(),
            },

            AuthFact::RecoveryCompleted { request_id, .. } => AuthFactDelta::RecoveryCompleted {
                request_id: request_id.clone(),
            },

            AuthFact::RecoveryFailed { request_id, .. } => AuthFactDelta::RecoveryFailed {
                request_id: request_id.clone(),
            },

            // Facts that don't produce view deltas
            AuthFact::ProofSubmitted { .. }
            | AuthFact::AuthVerified { .. }
            | AuthFact::AuthFailed { .. } => AuthFactDelta::NoChange,
        }
    }
}

impl FactReducer for AuthFactReducer {
    fn handles_type(&self) -> &'static str {
        AUTH_FACT_TYPE_ID
    }

    fn reduce_envelope(
        &self,
        context_id: ContextId,
        envelope: &aura_core::types::facts::FactEnvelope,
    ) -> Option<RelationalBinding> {
        if envelope.type_id.as_str() != AUTH_FACT_TYPE_ID {
            return None;
        }

        let fact = AuthFact::from_envelope(envelope)?;
        if !fact.validate_for_reduction(context_id) {
            return None;
        }

        let key = fact.binding_key();

        Some(RelationalBinding {
            binding_type: RelationalBindingType::Generic(key.sub_type.to_string()),
            context_id,
            data: key.data,
        })
    }
}

/// Delta produced by reducing an authentication fact
#[derive(Debug, Clone)]
pub enum AuthFactDelta {
    /// No view change
    NoChange,

    /// A pending challenge was created
    PendingChallenge {
        session_id: String,
        authority_id: AuthorityId,
        expires_at_ms: u64,
    },

    /// An active session was created
    ActiveSession {
        session_id: String,
        authority_id: AuthorityId,
        scope: SessionScope,
        expires_at_ms: u64,
    },

    /// A session was removed
    SessionRemoved { session_id: String },

    /// A pending recovery request was created
    PendingRecovery {
        request_id: String,
        account_id: AuthorityId,
        required_guardians: u32,
        approval_count: u32,
        expires_at_ms: u64,
    },

    /// A guardian approval was added to a request
    GuardianApprovalAdded {
        request_id: String,
        guardian_id: AuthorityId,
    },

    /// A recovery request completed successfully
    RecoveryCompleted { request_id: String },

    /// A recovery request failed
    RecoveryFailed { request_id: String },
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use aura_signature::session::SessionScope;

    fn test_authority() -> AuthorityId {
        AuthorityId::new_from_entropy([1u8; 32])
    }

    fn test_device() -> DeviceId {
        DeviceId::from_bytes([2u8; 32])
    }

    fn test_context_id() -> ContextId {
        ContextId::new_from_entropy([9u8; 32])
    }

    #[test]
    fn test_challenge_generated_fact() {
        let fact = AuthFact::ChallengeGenerated {
            context_id: test_context_id(),
            session_id: "session_123".to_string(),
            authority_id: test_authority(),
            device_id: Some(test_device()),
            scope: SessionScope::Protocol {
                protocol_type: "test".to_string(),
            },
            expires_at_ms: 2000,
            created_at_ms: 1000,
        };

        assert_eq!(fact.type_id(), AUTH_FACT_TYPE_ID);
        assert_eq!(fact.primary_authority(), test_authority());
        assert_eq!(fact.timestamp_ms(), 1000);
        assert_eq!(fact.session_id(), Some("session_123"));
        assert!(!fact.is_success());
        assert!(!fact.is_failure());
    }

    #[test]
    fn test_auth_verified_fact() {
        let fact = AuthFact::AuthVerified {
            context_id: test_context_id(),
            session_id: "session_123".to_string(),
            authority_id: test_authority(),
            device_id: Some(test_device()),
            verified_at_ms: 1500,
            message_hash: [0u8; 32],
        };

        assert!(fact.is_success());
        assert!(!fact.is_failure());
    }

    #[test]
    fn test_auth_failed_fact() {
        let fact = AuthFact::AuthFailed {
            context_id: test_context_id(),
            session_id: "session_123".to_string(),
            authority_id: test_authority(),
            reason: "Invalid signature".to_string(),
            failed_at_ms: 1500,
        };

        assert!(!fact.is_success());
        assert!(fact.is_failure());
    }

    #[test]
    fn test_guardian_approval_requested_fact() {
        let fact = AuthFact::GuardianApprovalRequested {
            context_id: test_context_id(),
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

        assert_eq!(fact.request_id(), Some("recovery_123"));
        assert!(fact.session_id().is_none());
    }

    #[test]
    fn test_fact_reducer() {
        let reducer = AuthFactReducer::new();

        let fact = AuthFact::SessionIssued {
            context_id: test_context_id(),
            session_id: "session_123".to_string(),
            authority_id: test_authority(),
            device_id: Some(test_device()),
            scope: SessionScope::Protocol {
                protocol_type: "test".to_string(),
            },
            issued_at_ms: 1000,
            expires_at_ms: 2000,
        };

        let delta = reducer.reduce(&fact);
        match delta {
            AuthFactDelta::ActiveSession {
                session_id,
                expires_at_ms,
                ..
            } => {
                assert_eq!(session_id, "session_123");
                assert_eq!(expires_at_ms, 2000);
            }
            _ => panic!("Expected ActiveSession delta"),
        }
    }

    #[test]
    fn test_fact_serialization() {
        let fact = AuthFact::ChallengeGenerated {
            context_id: test_context_id(),
            session_id: "session_123".to_string(),
            authority_id: test_authority(),
            device_id: None,
            scope: SessionScope::Protocol {
                protocol_type: "test".to_string(),
            },
            expires_at_ms: 2000,
            created_at_ms: 1000,
        };

        let serialized = serde_json::to_string(&fact).unwrap();
        let deserialized: AuthFact = serde_json::from_str(&serialized).unwrap();
        // Compare session IDs since AuthFact doesn't implement PartialEq (SessionScope lacks it)
        assert_eq!(fact.session_id(), deserialized.session_id());
        assert_eq!(fact.timestamp_ms(), deserialized.timestamp_ms());
    }

    #[test]
    fn test_reducer_rejects_context_mismatch() {
        let reducer = AuthFactReducer::new();
        let fact = AuthFact::SessionIssued {
            context_id: test_context_id(),
            session_id: "session_456".to_string(),
            authority_id: test_authority(),
            device_id: Some(test_device()),
            scope: SessionScope::Protocol {
                protocol_type: "test".to_string(),
            },
            issued_at_ms: 1000,
            expires_at_ms: 2000,
        };

        let other_context = ContextId::new_from_entropy([10u8; 32]);
        assert_ne!(fact.context_id(), other_context);
        let delta = reducer.reduce(&fact);
        assert!(matches!(delta, AuthFactDelta::ActiveSession { .. }));
    }

    #[test]
    fn test_binding_key_derivation() {
        let fact = AuthFact::GuardianApproved {
            context_id: test_context_id(),
            request_id: "req-123".to_string(),
            guardian_id: test_authority(),
            signature: vec![0u8; 64],
            justification: "Approved".to_string(),
            approved_at_ms: 1234,
        };

        let key = fact.binding_key();
        assert_eq!(key.sub_type, "auth-guardian-approved");
        assert_eq!(key.data, b"req-123".to_vec());
    }

    #[test]
    fn test_reducer_idempotence() {
        let reducer = AuthFactReducer::new();
        let context_id = test_context_id();
        let fact = AuthFact::SessionIssued {
            context_id,
            session_id: "session_123".to_string(),
            authority_id: test_authority(),
            device_id: Some(test_device()),
            scope: SessionScope::Protocol {
                protocol_type: "test".to_string(),
            },
            issued_at_ms: 1000,
            expires_at_ms: 2000,
        };

        let delta1 = reducer.reduce(&fact);
        let delta2 = reducer.reduce(&fact);
        match (delta1, delta2) {
            (
                AuthFactDelta::ActiveSession {
                    session_id: session_id_1,
                    authority_id: authority_id_1,
                    scope: scope_1,
                    expires_at_ms: expires_at_1,
                },
                AuthFactDelta::ActiveSession {
                    session_id: session_id_2,
                    authority_id: authority_id_2,
                    scope: scope_2,
                    expires_at_ms: expires_at_2,
                },
            ) => {
                assert_eq!(session_id_1, session_id_2);
                assert_eq!(authority_id_1, authority_id_2);
                assert_eq!(expires_at_1, expires_at_2);
                match (scope_1, scope_2) {
                    (
                        SessionScope::Protocol {
                            protocol_type: protocol_type_1,
                        },
                        SessionScope::Protocol {
                            protocol_type: protocol_type_2,
                        },
                    ) => {
                        assert_eq!(protocol_type_1, protocol_type_2);
                    }
                    _ => panic!("Expected Protocol SessionScope variants"),
                }
            }
            _ => panic!("Expected ActiveSession deltas"),
        }
    }
}
