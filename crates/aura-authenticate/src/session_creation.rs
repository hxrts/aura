//! Session Creation Choreography
//!
//! This module implements distributed session ticket creation and validation
//! using choreographic programming principles with the rumpsteak-aura framework.

use crate::AuraResult;
use aura_core::{AccountId, DeviceId};
use aura_macros::choreography;
use aura_protocol::AuraEffectSystem;
use aura_verify::session::{SessionScope, SessionTicket};
use aura_verify::VerifiedIdentity;
use serde::{Deserialize, Serialize};

/// Session creation request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionCreationRequest {
    /// Device requesting the session
    pub device_id: DeviceId,
    /// Account context
    pub account_id: AccountId,
    /// Verified identity from authentication
    pub verified_identity: VerifiedIdentity,
    /// Requested session scope
    pub requested_scope: SessionScope,
    /// Session duration in seconds
    pub duration_seconds: u64,
}

/// Session creation response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionCreationResponse {
    /// Created session ticket
    pub session_ticket: Option<SessionTicket>,
    /// Participating devices
    pub participants: Vec<DeviceId>,
    /// Success indicator
    pub success: bool,
    /// Error message if any
    pub error: Option<String>,
}

// Session creation Choreography Protocol
//
// This choreography implements distributed session creation with multi-party approval:
// - Requester submits session creation request to coordinator
// - Coordinator validates request and seeks approval from approvers
// - Approvers review and approve/reject the session request
// - Coordinator creates session ticket and returns result
choreography! {
    #[namespace = "session_creation"]
    protocol SessionCreationChoreography {
        roles: Requester, Approver, Coordinator;

        // Phase 1: Session Request
        // Requester initiates session creation with identity verification
        Requester[guard_capability = "request_session",
                  flow_cost = 100,
                  journal_facts = "session_request_submitted"]
        -> Coordinator: SessionRequest(SessionRequest);

        // Phase 2: Approval Request
        // Coordinator validates request and seeks approval
        Coordinator[guard_capability = "validate_session_request",
                    flow_cost = 150,
                    journal_facts = "session_request_validated"]
        -> Approver: ApprovalRequest(ApprovalRequest);

        // Phase 3: Approval Response
        choice Approver {
            approve: {
                // Approver approves the session request
                Approver[guard_capability = "approve_session",
                        flow_cost = 100,
                        journal_facts = "session_approved"]
                -> Coordinator: SessionApproved(SessionApproved);
            }
            reject: {
                // Approver rejects the session request
                Approver[guard_capability = "reject_session",
                        flow_cost = 75,
                        journal_facts = "session_rejected"]
                -> Coordinator: SessionRejected(SessionRejected);
            }
        }

        // Phase 4: Session Creation/Rejection Response
        choice Coordinator {
            success: {
                // Coordinator creates session ticket and returns success
                Coordinator[guard_capability = "create_session",
                           flow_cost = 200,
                           journal_facts = "session_created",
                           journal_merge = true]
                -> Requester: SessionCreated(SessionCreated);

                // Notify approver of successful session creation
                Coordinator[guard_capability = "notify_approver",
                           flow_cost = 50,
                           journal_facts = "session_creation_notified"]
                -> Approver: SessionCreated(SessionCreated);
            }
            failure: {
                // Coordinator returns failure response
                Coordinator[guard_capability = "reject_session_creation",
                           flow_cost = 100,
                           journal_facts = "session_creation_failed"]
                -> Requester: SessionCreationFailed(SessionCreationFailed);

                // Notify approver of failed session creation
                Coordinator[guard_capability = "notify_approver",
                           flow_cost = 50,
                           journal_facts = "session_creation_failed"]
                -> Approver: SessionCreationFailed(SessionCreationFailed);
            }
        }
    }
}

// Message types for session creation choreography

/// Session request message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionRequest {
    /// Device requesting the session
    pub device_id: DeviceId,
    /// Account context
    pub account_id: AccountId,
    /// Verified identity from authentication
    pub verified_identity: VerifiedIdentity,
    /// Requested session scope
    pub requested_scope: SessionScope,
    /// Session duration in seconds
    pub duration_seconds: u64,
    /// Session ID for tracking
    pub session_id: String,
}

/// Approval request message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalRequest {
    /// Session ID being reviewed
    pub session_id: String,
    /// Device requesting the session
    pub requester_device_id: DeviceId,
    /// Account context
    pub account_id: AccountId,
    /// Requested session scope
    pub requested_scope: SessionScope,
    /// Session duration in seconds
    pub duration_seconds: u64,
}

/// Session approved message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionApproved {
    /// Session ID being approved
    pub session_id: String,
    /// Approver device
    pub approver_device_id: DeviceId,
    /// Approval timestamp
    pub approved_at: u64,
}

/// Session rejected message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionRejected {
    /// Session ID being rejected
    pub session_id: String,
    /// Approver device
    pub approver_device_id: DeviceId,
    /// Rejection reason
    pub reason: String,
    /// Rejection timestamp
    pub rejected_at: u64,
}

/// Session created message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionCreated {
    /// Session ID that was created
    pub session_id: String,
    /// Created session ticket
    pub session_ticket: SessionTicket,
    /// Creation timestamp
    pub created_at: u64,
}

/// Session creation failed message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionCreationFailed {
    /// Session ID that failed
    pub session_id: String,
    /// Failure reason
    pub reason: String,
    /// Failure timestamp
    pub failed_at: u64,
}

// Placeholder function for session creation choreography access
// The choreography macro will generate the appropriate types and functions
pub fn get_session_creation_choreography() {
    // The choreography macro will generate the appropriate types and functions
}

/// Session creation coordinator using choreographic protocol
pub struct SessionCreationCoordinator {
    /// Local effect system
    effect_system: AuraEffectSystem,
}

impl SessionCreationCoordinator {
    /// Create new session creation coordinator
    pub fn new(effect_system: AuraEffectSystem) -> Self {
        Self { effect_system }
    }

    /// Create session using choreographic protocol
    pub async fn create_creation(
        &mut self,
        request: SessionCreationRequest,
    ) -> AuraResult<SessionCreationResponse> {
        tracing::info!(
            "Starting choreographic session created for device: {}",
            request.device_id
        );

        // TODO: Execute the choreographic protocol using the generated SessionCreationChoreography
        // This is a placeholder until the choreography macro is fully integrated

        // For now, return a basic response
        Ok(SessionCreationResponse {
            session_ticket: None,
            participants: vec![request.device_id],
            success: false,
            error: Some("Choreographic session created not yet fully implemented".to_string()),
        })
    }

    /// Get the current effect system
    pub fn effect_system(&self) -> &AuraEffectSystem {
        &self.effect_system
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::test_utils::test_device_id;
    use aura_core::{AccountId, DeviceId};
    use aura_macros::aura_test;
    use aura_verify::session::SessionScope;
    use aura_verify::VerifiedIdentity;

    #[test]
    fn test_choreography_creation() {
        get_session_creation_choreography();
        // Test that we can create the choreography instance successfully
        // The macro generates a struct with the protocol name
    }

    #[test]
    fn test_session_request_serialization() {
        let request = SessionRequest {
            device_id: test_device_id(1),
            account_id: AccountId::new(),
            verified_identity: VerifiedIdentity {
                proof: aura_verify::IdentityProof::Device {
                    device_id: test_device_id(1),
                    signature: [0u8; 64].into(),
                },
                message_hash: [0u8; 32],
            },
            requested_scope: SessionScope::Protocol {
                protocol_type: "session_auth".to_string(),
            },
            duration_seconds: 3600,
            session_id: "test_session".to_string(),
        };

        let serialized = serde_json::to_vec(&request).unwrap();
        let deserialized: SessionRequest = serde_json::from_slice(&serialized).unwrap();

        assert_eq!(request.session_id, deserialized.session_id);
        assert_eq!(request.device_id, deserialized.device_id);
        assert_eq!(request.duration_seconds, deserialized.duration_seconds);
    }

    #[aura_test]
    async fn test_coordinator_creation() -> aura_core::AuraResult<()> {
        let device_id = test_device_id(2);
        let fixture = aura_testkit::create_test_fixture_with_device_id(device_id).await?;
        let coordinator = SessionCreationCoordinator::new(fixture.effect_system());

        assert_eq!(coordinator.effect_system().device_id(), device_id);
        Ok(())
    }
}
