//! Authentication Service
//!
//! Main coordinator for authentication operations.
//! All operations flow through the guard chain and return outcomes
//! for the caller to execute effects.
//!
//! # Architecture
//!
//! The `AuthService` follows the same pattern as `aura-invitation::InvitationService`:
//!
//! 1. Caller prepares a `GuardSnapshot` asynchronously
//! 2. Service evaluates guards synchronously, returning `GuardOutcome`
//! 3. Caller executes `EffectCommand` items asynchronously
//!
//! This separation ensures:
//! - Guard evaluation is pure and testable
//! - Effect execution is explicit and controllable
//! - No I/O happens during guard evaluation
//!
//! # Migration from Coordinators
//!
//! This service replaces the legacy coordinator pattern (`DeviceAuthCoordinator`,
//! `SessionCreationCoordinator`, `GuardianAuthCoordinator`) with a unified
//! service that uses the guard chain pattern.

use crate::facts::AuthFact;
use crate::guards::{
    check_capability, check_flow_budget, costs, EffectCommand, GuardOutcome, GuardSnapshot,
    RecoveryContext, RecoveryOperationType,
};
use aura_core::hash::hash;
use aura_core::identifiers::{AuthorityId, ContextId};
use aura_journal::DomainFact;
use aura_signature::session::SessionScope;
use serde::{Deserialize, Serialize};

#[derive(Debug, thiserror::Error)]
enum AuthGuardError {
    #[error("Session duration {requested}s exceeds maximum {max}s")]
    SessionDurationTooLong { requested: u64, max: u64 },
    #[error("Guardian set modification requires recovery:approve capability")]
    GuardianSetRequiresApproveCapability,
    #[error("Emergency freeze requires recovery:initiate capability or emergency flag")]
    EmergencyFreezeRequiresInitiateCapability,
}

// =============================================================================
// Service Configuration
// =============================================================================

/// Configuration for the authentication service
#[derive(Debug, Clone)]
pub struct AuthServiceConfig {
    /// Default challenge expiration time in milliseconds
    pub challenge_expiration_ms: u64,

    /// Maximum session duration in seconds
    pub max_session_duration_secs: u64,

    /// Default guardian approval expiration in milliseconds
    pub guardian_approval_expiration_ms: u64,

    /// Whether to require explicit capability for recovery operations
    pub require_recovery_capability: bool,
}

impl Default for AuthServiceConfig {
    fn default() -> Self {
        Self {
            challenge_expiration_ms: 5 * 60 * 1000,  // 5 minutes
            max_session_duration_secs: 24 * 60 * 60, // 24 hours
            guardian_approval_expiration_ms: 7 * 24 * 60 * 60 * 1000, // 7 days
            require_recovery_capability: true,
        }
    }
}

#[derive(Debug, Clone)]
struct AuthPolicy {
    #[allow(dead_code)] // Reserved for future policy enforcement
    context_id: ContextId,
    max_session_duration_secs: u64,
    require_recovery_capability: bool,
}

impl AuthPolicy {
    fn for_snapshot(config: &AuthServiceConfig, snapshot: &GuardSnapshot) -> Self {
        Self {
            context_id: derive_auth_context_id(snapshot),
            max_session_duration_secs: config.max_session_duration_secs,
            require_recovery_capability: config.require_recovery_capability,
        }
    }
}

fn derive_auth_context_id(snapshot: &GuardSnapshot) -> ContextId {
    snapshot
        .context_id
        .unwrap_or_else(|| ContextId::new_from_entropy(hash(&snapshot.authority_id.to_bytes())))
}

// =============================================================================
// Authentication Service
// =============================================================================

/// Main authentication service
///
/// Provides pure, synchronous guard evaluation for authentication operations.
/// All effect execution is deferred to the caller via `EffectCommand`.
#[derive(Debug, Clone)]
pub struct AuthService {
    config: AuthServiceConfig,
}

impl AuthService {
    /// Create a new authentication service with default configuration
    pub fn new() -> Self {
        Self {
            config: AuthServiceConfig::default(),
        }
    }

    /// Create a new authentication service with custom configuration
    pub fn with_config(config: AuthServiceConfig) -> Self {
        Self { config }
    }

    // =========================================================================
    // Challenge Operations
    // =========================================================================

    /// Request an authentication challenge
    ///
    /// Returns a `GuardOutcome` containing effect commands to generate
    /// the challenge if allowed.
    pub fn request_challenge(&self, snapshot: &GuardSnapshot, scope: SessionScope) -> GuardOutcome {
        // Check capability
        if let Some(outcome) = check_capability(snapshot, costs::CAP_REQUEST_AUTH) {
            return outcome;
        }

        // Check budget
        if let Some(outcome) = check_flow_budget(snapshot, costs::CHALLENGE_REQUEST_COST) {
            return outcome;
        }

        let session_id = generate_session_id(snapshot);
        let expires_at_ms = snapshot.now_ms + self.config.challenge_expiration_ms;

        let context_id = derive_auth_context_id(snapshot);
        let fact = AuthFact::ChallengeGenerated {
            context_id,
            session_id: session_id.clone(),
            authority_id: snapshot.authority_id,
            device_id: snapshot.device_id,
            scope,
            expires_at_ms,
            created_at_ms: snapshot.now_ms,
        };
        let fact_data = fact.to_bytes();

        GuardOutcome::allowed(vec![
            EffectCommand::ChargeFlowBudget {
                cost: costs::CHALLENGE_REQUEST_COST,
            },
            EffectCommand::GenerateChallenge {
                session_id,
                expires_at_ms,
            },
            EffectCommand::JournalAppend {
                fact_type: crate::facts::AUTH_FACT_TYPE_ID.to_string(),
                fact_data,
            },
        ])
    }

    // =========================================================================
    // Proof Submission
    // =========================================================================

    /// Submit an identity proof for verification
    ///
    /// Returns a `GuardOutcome` containing effect commands to process
    /// the proof if allowed.
    pub fn submit_proof(
        &self,
        snapshot: &GuardSnapshot,
        session_id: String,
        proof_hash: [u8; 32],
    ) -> GuardOutcome {
        // Check capability
        if let Some(outcome) = check_capability(snapshot, costs::CAP_SUBMIT_PROOF) {
            return outcome;
        }

        // Check budget
        if let Some(outcome) = check_flow_budget(snapshot, costs::PROOF_SUBMISSION_COST) {
            return outcome;
        }

        let context_id = derive_auth_context_id(snapshot);
        let fact = AuthFact::ProofSubmitted {
            context_id,
            session_id: session_id.clone(),
            authority_id: snapshot.authority_id,
            proof_hash,
            submitted_at_ms: snapshot.now_ms,
        };
        let fact_data = fact.to_bytes();

        GuardOutcome::allowed(vec![
            EffectCommand::ChargeFlowBudget {
                cost: costs::PROOF_SUBMISSION_COST,
            },
            EffectCommand::JournalAppend {
                fact_type: crate::facts::AUTH_FACT_TYPE_ID.to_string(),
                fact_data,
            },
            EffectCommand::RecordReceipt {
                operation: format!("proof_submission:{session_id}"),
                peer: None,
                timestamp_ms: snapshot.now_ms,
            },
        ])
    }

    // =========================================================================
    // Session Creation
    // =========================================================================

    /// Create a session ticket after successful authentication
    ///
    /// Returns a `GuardOutcome` containing effect commands to issue
    /// the session ticket if allowed.
    pub fn create_session(
        &self,
        snapshot: &GuardSnapshot,
        scope: SessionScope,
        duration_seconds: u64,
    ) -> GuardOutcome {
        let policy = AuthPolicy::for_snapshot(&self.config, snapshot);
        // Check capability
        if let Some(outcome) = check_capability(snapshot, costs::CAP_CREATE_SESSION) {
            return outcome;
        }

        // Check budget
        if let Some(outcome) = check_flow_budget(snapshot, costs::SESSION_CREATION_COST) {
            return outcome;
        }

        // Check duration
        if duration_seconds > policy.max_session_duration_secs {
            return GuardOutcome::denied(
                AuthGuardError::SessionDurationTooLong {
                    requested: duration_seconds,
                    max: policy.max_session_duration_secs,
                }
                .to_string(),
            );
        }

        let session_id = generate_session_id(snapshot);
        let expires_at_ms = snapshot.now_ms + (duration_seconds * 1000);

        let context_id = derive_auth_context_id(snapshot);
        let fact = AuthFact::SessionIssued {
            context_id,
            session_id: session_id.clone(),
            authority_id: snapshot.authority_id,
            device_id: snapshot.device_id,
            scope: scope.clone(),
            issued_at_ms: snapshot.now_ms,
            expires_at_ms,
        };
        let fact_data = fact.to_bytes();

        GuardOutcome::allowed(vec![
            EffectCommand::ChargeFlowBudget {
                cost: costs::SESSION_CREATION_COST,
            },
            EffectCommand::IssueSessionTicket {
                session_id,
                scope,
                expires_at_ms,
            },
            EffectCommand::JournalAppend {
                fact_type: crate::facts::AUTH_FACT_TYPE_ID.to_string(),
                fact_data,
            },
        ])
    }

    // =========================================================================
    // Guardian Approval
    // =========================================================================

    /// Request guardian approval for a recovery operation
    ///
    /// Returns a `GuardOutcome` containing effect commands to initiate
    /// the guardian approval process if allowed.
    pub fn request_guardian_approval(
        &self,
        snapshot: &GuardSnapshot,
        account_id: AuthorityId,
        context: RecoveryContext,
        required_guardians: u32,
    ) -> GuardOutcome {
        let policy = AuthPolicy::for_snapshot(&self.config, snapshot);
        // Check capability
        if let Some(outcome) = check_capability(snapshot, costs::CAP_REQUEST_GUARDIAN_APPROVAL) {
            return outcome;
        }

        // Check budget
        if let Some(outcome) = check_flow_budget(snapshot, costs::GUARDIAN_APPROVAL_REQUEST_COST) {
            return outcome;
        }

        // Check recovery operation type constraints
        if policy.require_recovery_capability {
            match context.operation_type {
                RecoveryOperationType::GuardianSetModification => {
                    if !snapshot.has_capability(costs::CAP_APPROVE_RECOVERY) {
                        return GuardOutcome::denied(
                            AuthGuardError::GuardianSetRequiresApproveCapability.to_string(),
                        );
                    }
                }
                RecoveryOperationType::EmergencyFreeze if !context.is_emergency => {
                    if !snapshot.has_capability(costs::CAP_INITIATE_RECOVERY) {
                        return GuardOutcome::denied(
                            AuthGuardError::EmergencyFreezeRequiresInitiateCapability.to_string(),
                        );
                    }
                }
                _ => {}
            }
        }

        let request_id = generate_request_id(snapshot);
        let expires_at_ms = snapshot.now_ms + self.config.guardian_approval_expiration_ms;

        let context_id = derive_auth_context_id(snapshot);
        let fact = AuthFact::GuardianApprovalRequested {
            context_id,
            request_id: request_id.clone(),
            account_id,
            requester_id: snapshot.authority_id,
            operation_type: context.operation_type.clone(),
            required_guardians,
            is_emergency: context.is_emergency,
            justification: context.justification,
            requested_at_ms: snapshot.now_ms,
            expires_at_ms,
        };
        let fact_data = fact.to_bytes();

        GuardOutcome::allowed(vec![
            EffectCommand::ChargeFlowBudget {
                cost: costs::GUARDIAN_APPROVAL_REQUEST_COST,
            },
            EffectCommand::JournalAppend {
                fact_type: crate::facts::AUTH_FACT_TYPE_ID.to_string(),
                fact_data,
            },
            EffectCommand::AggregateGuardianApprovals {
                request_id: request_id.clone(),
                threshold: required_guardians,
            },
            EffectCommand::NotifyPeer {
                peer: account_id,
                event_type: "guardian_approval_request".to_string(),
                event_data: request_id.into_bytes(),
            },
        ])
    }

    /// Submit a guardian approval decision
    ///
    /// Returns a `GuardOutcome` containing effect commands to record
    /// the decision if allowed.
    pub fn submit_guardian_decision(
        &self,
        snapshot: &GuardSnapshot,
        request_id: String,
        approved: bool,
        justification: String,
        signature: Vec<u8>,
    ) -> GuardOutcome {
        // Check capability
        if let Some(outcome) = check_capability(snapshot, costs::CAP_APPROVE_GUARDIAN) {
            return outcome;
        }

        // Check budget
        if let Some(outcome) = check_flow_budget(snapshot, costs::GUARDIAN_APPROVAL_DECISION_COST) {
            return outcome;
        }

        let context_id = derive_auth_context_id(snapshot);
        let fact = if approved {
            AuthFact::GuardianApproved {
                context_id,
                request_id: request_id.clone(),
                guardian_id: snapshot.authority_id,
                signature,
                justification,
                approved_at_ms: snapshot.now_ms,
            }
        } else {
            AuthFact::GuardianDenied {
                context_id,
                request_id: request_id.clone(),
                guardian_id: snapshot.authority_id,
                reason: justification,
                denied_at_ms: snapshot.now_ms,
            }
        };
        let fact_data = fact.to_bytes();

        GuardOutcome::allowed(vec![
            EffectCommand::ChargeFlowBudget {
                cost: costs::GUARDIAN_APPROVAL_DECISION_COST,
            },
            EffectCommand::JournalAppend {
                fact_type: crate::facts::AUTH_FACT_TYPE_ID.to_string(),
                fact_data,
            },
            EffectCommand::RecordReceipt {
                operation: format!("guardian_decision:{request_id}:{approved}"),
                peer: Some(snapshot.authority_id),
                timestamp_ms: snapshot.now_ms,
            },
        ])
    }

    // =========================================================================
    // Session Revocation
    // =========================================================================

    /// Revoke an active session
    ///
    /// Returns a `GuardOutcome` containing effect commands to revoke
    /// the session if allowed.
    pub fn revoke_session(
        &self,
        snapshot: &GuardSnapshot,
        session_id: String,
        reason: String,
    ) -> GuardOutcome {
        // Session revocation uses the create_session capability
        if let Some(outcome) = check_capability(snapshot, costs::CAP_CREATE_SESSION) {
            return outcome;
        }

        let context_id = derive_auth_context_id(snapshot);
        let fact = AuthFact::SessionRevoked {
            context_id,
            session_id: session_id.clone(),
            revoked_by: snapshot.authority_id,
            reason,
            revoked_at_ms: snapshot.now_ms,
        };
        let fact_data = fact.to_bytes();

        GuardOutcome::allowed(vec![
            EffectCommand::JournalAppend {
                fact_type: crate::facts::AUTH_FACT_TYPE_ID.to_string(),
                fact_data,
            },
            EffectCommand::RecordReceipt {
                operation: format!("session_revocation:{session_id}"),
                peer: None,
                timestamp_ms: snapshot.now_ms,
            },
        ])
    }
}

impl Default for AuthService {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// Helper Functions
// =============================================================================

/// Generate a short hex representation of an AuthorityId
fn authority_hex_short(id: AuthorityId) -> String {
    let bytes = id.to_bytes();
    format!(
        "{:02x}{:02x}{:02x}{:02x}",
        bytes[0], bytes[1], bytes[2], bytes[3]
    )
}

/// Generate a unique session ID based on the snapshot
fn generate_session_id(snapshot: &GuardSnapshot) -> String {
    format!(
        "session_{}_{}_{}",
        authority_hex_short(snapshot.authority_id),
        snapshot.epoch,
        snapshot.now_ms
    )
}

/// Generate a unique request ID for guardian approval
fn generate_request_id(snapshot: &GuardSnapshot) -> String {
    format!(
        "guardian_req_{}_{}_{}",
        authority_hex_short(snapshot.authority_id),
        snapshot.epoch,
        snapshot.now_ms
    )
}

// =============================================================================
// Result Types
// =============================================================================

/// Result of a challenge request operation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChallengeResult {
    /// Session ID for the challenge
    pub session_id: String,
    /// Challenge bytes to sign
    pub challenge: Vec<u8>,
    /// Challenge expiration timestamp (ms)
    pub expires_at_ms: u64,
    /// Whether the operation succeeded
    pub success: bool,
    /// Error message if failed
    pub error: Option<String>,
}

/// Result of a session creation operation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionResult {
    /// Session ticket ID
    pub session_id: String,
    /// Session expiration timestamp (ms)
    pub expires_at_ms: u64,
    /// Whether the operation succeeded
    pub success: bool,
    /// Error message if failed
    pub error: Option<String>,
}

/// Result of a guardian approval request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuardianApprovalResult {
    /// Request ID
    pub request_id: String,
    /// Number of approvals received
    pub approval_count: u32,
    /// Required approvals
    pub required_count: u32,
    /// Whether threshold was met
    pub threshold_met: bool,
    /// Whether the operation succeeded
    pub success: bool,
    /// Error message if failed
    pub error: Option<String>,
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::FlowCost;

    fn test_authority() -> AuthorityId {
        AuthorityId::new_from_entropy([1u8; 32])
    }

    fn test_snapshot() -> GuardSnapshot {
        GuardSnapshot::new(
            test_authority(),
            None,
            None,
            FlowCost::new(100),
            vec![
                costs::CAP_REQUEST_AUTH.to_string(),
                costs::CAP_SUBMIT_PROOF.to_string(),
                costs::CAP_CREATE_SESSION.to_string(),
                costs::CAP_REQUEST_GUARDIAN_APPROVAL.to_string(),
                costs::CAP_APPROVE_GUARDIAN.to_string(),
            ],
            1,
            1000,
        )
    }

    #[test]
    fn test_service_creation() {
        let service = AuthService::new();
        assert_eq!(service.config.challenge_expiration_ms, 5 * 60 * 1000);
    }

    #[test]
    fn test_request_challenge_allowed() {
        let service = AuthService::new();
        let snapshot = test_snapshot();
        let scope = SessionScope::Protocol {
            protocol_type: "test".to_string(),
        };

        let outcome = service.request_challenge(&snapshot, scope);
        assert!(outcome.is_allowed());
        assert!(!outcome.effects.is_empty());
    }

    #[test]
    fn test_request_challenge_missing_capability() {
        let service = AuthService::new();
        let snapshot = GuardSnapshot::new(
            test_authority(),
            None,
            None,
            FlowCost::new(100),
            vec![], // No capabilities
            1,
            1000,
        );
        let scope = SessionScope::Protocol {
            protocol_type: "test".to_string(),
        };

        let outcome = service.request_challenge(&snapshot, scope);
        assert!(outcome.is_denied());
    }

    #[test]
    fn test_create_session_duration_exceeded() {
        let service = AuthService::new();
        let snapshot = test_snapshot();
        let scope = SessionScope::Protocol {
            protocol_type: "test".to_string(),
        };

        let outcome = service.create_session(&snapshot, scope, 100_000); // > 24 hours
        assert!(outcome.is_denied());
    }

    #[test]
    fn test_create_session_allowed() {
        let service = AuthService::new();
        let snapshot = test_snapshot();
        let scope = SessionScope::Protocol {
            protocol_type: "test".to_string(),
        };

        let outcome = service.create_session(&snapshot, scope, 3600); // 1 hour
        assert!(outcome.is_allowed());
    }

    #[test]
    fn test_submit_proof_allowed() {
        let service = AuthService::new();
        let snapshot = test_snapshot();

        let outcome = service.submit_proof(&snapshot, "session_123".to_string(), [0u8; 32]);
        assert!(outcome.is_allowed());
    }

    #[test]
    fn test_guardian_approval_request() {
        let service = AuthService::new();
        let snapshot = test_snapshot();
        let context = RecoveryContext::new(
            RecoveryOperationType::DeviceKeyRecovery,
            "Lost device",
            1000,
        );

        let outcome = service.request_guardian_approval(&snapshot, test_authority(), context, 2);
        assert!(outcome.is_allowed());
    }

    #[test]
    fn test_guardian_decision_approved() {
        let service = AuthService::new();
        let snapshot = test_snapshot();

        let outcome = service.submit_guardian_decision(
            &snapshot,
            "request_123".to_string(),
            true,
            "Approved recovery".to_string(),
            vec![0u8; 64],
        );
        assert!(outcome.is_allowed());
    }

    #[test]
    fn test_guardian_decision_denied() {
        let service = AuthService::new();
        let snapshot = test_snapshot();

        let outcome = service.submit_guardian_decision(
            &snapshot,
            "request_123".to_string(),
            false,
            "Suspicious request".to_string(),
            vec![],
        );
        assert!(outcome.is_allowed());
    }

    #[test]
    fn test_revoke_session() {
        let service = AuthService::new();
        let snapshot = test_snapshot();

        let outcome = service.revoke_session(
            &snapshot,
            "session_123".to_string(),
            "User requested".to_string(),
        );
        assert!(outcome.is_allowed());
    }
}
