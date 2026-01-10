//! Invitation Protocol Definitions
//!
//! MPST choreography definitions for invitation exchange and guardian invitation.
//! These define the message flow and guard annotations for invitation ceremonies.

use crate::InvitationType;
use aura_core::identifiers::{AuthorityId, CeremonyId, InvitationId};
use aura_core::DeviceId;
use serde::{Deserialize, Serialize};

// =============================================================================
// Protocol Message Types
// =============================================================================

/// Invitation offer message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InvitationOffer {
    /// Unique invitation identifier
    pub invitation_id: InvitationId,
    /// Type of invitation (device, guardian, channel, etc.)
    pub invitation_type: InvitationType,
    /// Sender of the invitation
    pub sender: AuthorityId,
    /// Optional message included with invitation
    pub message: Option<String>,
    /// Expiration timestamp in milliseconds
    pub expires_at_ms: Option<u64>,
    /// Cryptographic commitment to invitation terms
    pub commitment: [u8; 32],
}

/// Invitation response message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InvitationResponse {
    /// Invitation identifier being responded to
    pub invitation_id: InvitationId,
    /// Whether the invitation was accepted
    pub accepted: bool,
    /// Optional response message
    pub message: Option<String>,
    /// Responder signature over acceptance/decline
    pub signature: Vec<u8>,
}

/// Invitation acknowledgment message (confirms response received)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InvitationAck {
    /// Invitation identifier
    pub invitation_id: InvitationId,
    /// Whether the response was successfully processed
    pub success: bool,
    /// Result status (e.g., "relationship_established", "declined_noted")
    pub status: String,
}

/// Guardian invitation request (specialized for guardian relationships)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuardianRequest {
    /// Unique invitation identifier
    pub invitation_id: InvitationId,
    /// Principal requesting guardian relationship
    pub principal: AuthorityId,
    /// Proposed guardian role description
    pub role_description: String,
    /// Recovery capabilities being granted
    pub recovery_capabilities: Vec<String>,
    /// Expiration timestamp
    pub expires_at_ms: Option<u64>,
}

/// Guardian acceptance response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuardianAccept {
    /// Invitation identifier
    pub invitation_id: InvitationId,
    /// Guardian's acceptance signature
    pub signature: Vec<u8>,
    /// Guardian's public key for recovery operations
    pub recovery_public_key: Vec<u8>,
}

/// Guardian decline response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuardianDecline {
    /// Invitation identifier
    pub invitation_id: InvitationId,
    /// Optional reason for declining
    pub reason: Option<String>,
}

/// Guardian confirmation (finalizes relationship)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuardianConfirm {
    /// Invitation identifier
    pub invitation_id: InvitationId,
    /// Relationship established successfully
    pub established: bool,
    /// Resulting relationship identifier
    pub relationship_id: Option<String>,
}

/// Device enrollment invitation request (adds a device to an account authority).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceEnrollmentRequest {
    /// Unique invitation identifier
    pub invitation_id: InvitationId,
    /// Account authority being modified
    pub subject_authority: AuthorityId,
    /// Ceremony identifier for the key rotation
    pub ceremony_id: CeremonyId,
    /// Pending epoch created during prepare
    pub pending_epoch: u64,
    /// Device id being enrolled
    pub device_id: DeviceId,
}

/// Device enrollment acceptance response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceEnrollmentAccept {
    /// Invitation identifier being accepted
    pub invitation_id: InvitationId,
    /// Ceremony identifier
    pub ceremony_id: CeremonyId,
    /// Device id that accepted and installed the share
    pub device_id: DeviceId,
}

/// Device enrollment confirmation (finalizes the enrollment).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceEnrollmentConfirm {
    /// Invitation identifier
    pub invitation_id: InvitationId,
    /// Ceremony identifier
    pub ceremony_id: CeremonyId,
    /// Whether enrollment was successfully established
    pub established: bool,
    /// Resulting epoch after enrollment (if successful)
    pub new_epoch: Option<u64>,
}

// =============================================================================
// Guard Cost Constants
// =============================================================================

/// Guard annotations module for flow costs and capabilities
pub mod guards {
    /// Flow cost for sending an invitation
    pub const INVITATION_SEND_COST: u32 = 1;

    /// Flow cost for responding to an invitation
    pub const INVITATION_RESPOND_COST: u32 = 1;

    /// Flow cost for acknowledgment
    pub const INVITATION_ACK_COST: u32 = 1;

    /// Flow cost for guardian request
    pub const GUARDIAN_REQUEST_COST: u32 = 2;

    /// Flow cost for guardian response
    pub const GUARDIAN_RESPOND_COST: u32 = 2;

    /// Flow cost for guardian confirmation
    pub const GUARDIAN_CONFIRM_COST: u32 = 1;

    /// Required capability for sending invitations
    pub const CAP_INVITATION_SEND: &str = "invitation:send";

    /// Required capability for accepting invitations
    pub const CAP_INVITATION_ACCEPT: &str = "invitation:accept";

    /// Required capability for declining invitations
    pub const CAP_INVITATION_DECLINE: &str = "invitation:decline";

    /// Required capability for guardian invitations
    pub const CAP_GUARDIAN_INVITE: &str = "invitation:guardian";

    /// Required capability for accepting guardian role
    pub const CAP_GUARDIAN_ACCEPT: &str = "invitation:guardian:accept";

    /// Flow cost for device enrollment request
    pub const DEVICE_ENROLL_REQUEST_COST: u32 = 2;

    /// Flow cost for device enrollment response
    pub const DEVICE_ENROLL_RESPOND_COST: u32 = 2;

    /// Flow cost for device enrollment confirmation
    pub const DEVICE_ENROLL_CONFIRM_COST: u32 = 1;

    /// Required capability for device enrollment
    pub const CAP_DEVICE_ENROLL: &str = "invitation:device:enroll";

    /// Required capability for accepting device enrollment
    pub const CAP_DEVICE_ACCEPT: &str = "invitation:device:accept";
}

// =============================================================================
// Choreography Protocol Definitions
// =============================================================================

/// Basic invitation exchange protocol module
pub mod exchange {
    #![allow(unused_imports)]
    use super::*;
    use aura_macros::choreography;

    // Invitation exchange choreography for basic invitation flow
    //
    // This choreography implements a simple invitation ceremony:
    // 1. Sender creates and sends invitation offer
    // 2. Receiver accepts or declines the invitation
    // 3. Sender acknowledges the response
    choreography!(include_str!("src/protocol.invitation_exchange.choreo"));
}

/// Guardian invitation protocol module
pub mod guardian {
    #![allow(unused_imports)]
    use super::*;
    use aura_macros::choreography;

    // Guardian invitation choreography for establishing guardian relationships
    //
    // This choreography implements the guardian invitation ceremony:
    // 1. Principal requests guardian relationship
    // 2. Guardian accepts or declines with appropriate response
    // 3. Principal confirms the relationship establishment
    choreography!(include_str!("src/protocol.guardian_invitation.choreo"));
}

/// Device enrollment protocol module
pub mod device_enrollment {
    #![allow(unused_imports)]
    use super::*;
    use aura_macros::choreography;

    // Device enrollment choreography for adding devices to an authority
    //
    // This choreography implements the device enrollment ceremony:
    // 1. Initiator (existing device) sends enrollment request with key package
    // 2. Invitee (new device) accepts and installs their share
    // 3. Initiator confirms the enrollment completion
    //
    // Note: The new device must create its own authority first, making it
    // addressable before the enrollment choreography can proceed.
    choreography!(include_str!("src/protocol.device_enrollment.choreo"));
}

// =============================================================================
// Protocol State Types
// =============================================================================

/// State of the basic invitation exchange protocol
#[derive(Debug, Clone)]
pub enum InvitationExchangeState {
    /// Initial state - no invitation sent
    Initial,
    /// Invitation offer sent, awaiting response
    OfferSent,
    /// Response received (accepted or declined)
    ResponseReceived { accepted: bool },
    /// Acknowledgment sent, protocol complete
    Complete { accepted: bool },
    /// Protocol failed
    Failed { reason: String },
}

/// State of the guardian invitation protocol
#[derive(Debug, Clone)]
pub enum GuardianInvitationState {
    /// Initial state
    Initial,
    /// Guardian request sent
    RequestSent,
    /// Guardian accepted
    Accepted { recovery_public_key: Vec<u8> },
    /// Guardian declined
    Declined { reason: Option<String> },
    /// Relationship confirmed and established
    Confirmed { relationship_id: String },
    /// Protocol failed
    Failed { reason: String },
}

/// State of the device enrollment protocol
#[derive(Debug, Clone)]
pub enum DeviceEnrollmentState {
    /// Initial state
    Initial,
    /// Enrollment request sent
    RequestSent,
    /// Enrollment accepted by invitee
    Accepted { device_id: String },
    /// Enrollment declined by invitee
    Declined { reason: Option<String> },
    /// Enrollment confirmed and established
    Confirmed { new_epoch: u64 },
    /// Protocol failed
    Failed { reason: String },
}

// =============================================================================
// Protocol Metadata
// =============================================================================

// =============================================================================
// Generated Runner Re-exports for execute_as Pattern
// =============================================================================

/// Re-exports for InvitationExchange choreography runners
pub mod exchange_runners {
    pub use super::exchange::rumpsteak_session_types_invitation::invitation::runners::{
        execute_as, run_receiver, run_sender, ReceiverOutput, SenderOutput,
    };
    pub use super::exchange::rumpsteak_session_types_invitation::invitation::InvitationExchangeRole;
}

/// Re-exports for GuardianInvitation choreography runners
pub mod guardian_runners {
    pub use super::guardian::rumpsteak_session_types_invitation_guardian::invitation_guardian::GuardianInvitationRole;
    pub use super::guardian::rumpsteak_session_types_invitation_guardian::invitation_guardian::runners::{
        execute_as, run_guardian, run_principal, GuardianOutput, PrincipalOutput,
    };
}

/// Re-exports for DeviceEnrollment choreography runners
pub mod device_enrollment_runners {
    pub use super::device_enrollment::rumpsteak_session_types_invitation_device_enrollment::invitation_device_enrollment::DeviceEnrollmentRole;
    pub use super::device_enrollment::rumpsteak_session_types_invitation_device_enrollment::invitation_device_enrollment::runners::{
        execute_as, run_initiator, run_invitee, InitiatorOutput, InviteeOutput,
    };
}

// =============================================================================
// Protocol Metadata
// =============================================================================

/// Protocol namespace for invitations
pub const PROTOCOL_NAMESPACE: &str = "invitation";

/// Protocol version
pub const PROTOCOL_VERSION: u32 = 1;

/// Protocol identifier for basic exchange
pub const EXCHANGE_PROTOCOL_ID: &str = "invitation.exchange.v1";

/// Protocol identifier for guardian invitation
pub const GUARDIAN_PROTOCOL_ID: &str = "invitation_guardian.guardian.v1";

/// Protocol identifier for device enrollment
pub const DEVICE_ENROLLMENT_PROTOCOL_ID: &str = "invitation_device_enrollment.device_enrollment.v1";

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::identifiers::{AuthorityId, CeremonyId, InvitationId};
    use aura_core::util::serialization::{from_slice, to_vec};

    fn test_authority() -> AuthorityId {
        AuthorityId::new_from_entropy([1u8; 32])
    }

    #[test]
    fn test_invitation_offer_serialization() {
        let offer = InvitationOffer {
            invitation_id: InvitationId::new("inv-123"),
            invitation_type: InvitationType::Contact { nickname: None },
            sender: test_authority(),
            message: Some("Please join".to_string()),
            expires_at_ms: Some(1000000),
            commitment: [42u8; 32],
        };

        let bytes = to_vec(&offer).unwrap();
        let restored: InvitationOffer = from_slice(&bytes).unwrap();

        assert_eq!(restored.invitation_id.as_str(), "inv-123");
        assert!(matches!(
            restored.invitation_type,
            InvitationType::Contact { nickname: None }
        ));
        assert_eq!(restored.message, Some("Please join".to_string()));
    }

    #[test]
    fn test_invitation_response_serialization() {
        let response = InvitationResponse {
            invitation_id: InvitationId::new("inv-123"),
            accepted: true,
            message: Some("Happy to join".to_string()),
            signature: vec![1, 2, 3, 4],
        };

        let bytes = to_vec(&response).unwrap();
        let restored: InvitationResponse = from_slice(&bytes).unwrap();

        assert_eq!(restored.invitation_id.as_str(), "inv-123");
        assert!(restored.accepted);
    }

    #[test]
    fn test_guardian_request_serialization() {
        let request = GuardianRequest {
            invitation_id: InvitationId::new("guard-456"),
            principal: test_authority(),
            role_description: "Primary guardian".to_string(),
            recovery_capabilities: vec!["recover:device".to_string()],
            expires_at_ms: Some(2000000),
        };

        let bytes = to_vec(&request).unwrap();
        let restored: GuardianRequest = from_slice(&bytes).unwrap();

        assert_eq!(restored.invitation_id.as_str(), "guard-456");
        assert_eq!(restored.role_description, "Primary guardian");
    }

    #[test]
    fn test_guardian_accept_serialization() {
        let accept = GuardianAccept {
            invitation_id: InvitationId::new("guard-456"),
            signature: vec![5, 6, 7, 8],
            recovery_public_key: vec![9, 10, 11, 12],
        };

        let bytes = to_vec(&accept).unwrap();
        let restored: GuardianAccept = from_slice(&bytes).unwrap();

        assert_eq!(restored.invitation_id.as_str(), "guard-456");
        assert_eq!(restored.recovery_public_key, vec![9, 10, 11, 12]);
    }

    #[test]
    #[allow(unused_assignments)]
    fn test_exchange_state_transitions() {
        let mut state = InvitationExchangeState::Initial;

        state = InvitationExchangeState::OfferSent;
        assert!(matches!(state, InvitationExchangeState::OfferSent));

        state = InvitationExchangeState::ResponseReceived { accepted: true };
        if let InvitationExchangeState::ResponseReceived { accepted } = state {
            assert!(accepted);
        }

        state = InvitationExchangeState::Complete { accepted: true };
        assert!(matches!(
            state,
            InvitationExchangeState::Complete { accepted: true }
        ));
    }

    #[test]
    #[allow(unused_assignments)]
    fn test_guardian_state_transitions() {
        let mut state = GuardianInvitationState::Initial;

        state = GuardianInvitationState::RequestSent;
        assert!(matches!(state, GuardianInvitationState::RequestSent));

        state = GuardianInvitationState::Accepted {
            recovery_public_key: vec![1, 2, 3],
        };
        if let GuardianInvitationState::Accepted {
            recovery_public_key,
        } = &state
        {
            assert_eq!(recovery_public_key, &vec![1, 2, 3]);
        }

        state = GuardianInvitationState::Confirmed {
            relationship_id: "rel-789".to_string(),
        };
        if let GuardianInvitationState::Confirmed { relationship_id } = state {
            assert_eq!(relationship_id, "rel-789");
        }
    }

    #[test]
    fn test_guard_constants() {
        assert_eq!(guards::INVITATION_SEND_COST, 1);
        assert_eq!(guards::GUARDIAN_REQUEST_COST, 2);
        assert_eq!(guards::CAP_INVITATION_SEND, "invitation:send");
        assert_eq!(guards::CAP_GUARDIAN_INVITE, "invitation:guardian");
    }

    #[test]
    fn test_protocol_metadata() {
        assert_eq!(PROTOCOL_NAMESPACE, "invitation");
        assert_eq!(PROTOCOL_VERSION, 1);
        assert!(EXCHANGE_PROTOCOL_ID.contains("invitation"));
        assert!(GUARDIAN_PROTOCOL_ID.contains("guardian"));
    }

    #[test]
    fn test_invitation_ack_serialization() {
        let ack = InvitationAck {
            invitation_id: InvitationId::new("inv-123"),
            success: true,
            status: "relationship_established".to_string(),
        };

        let bytes = to_vec(&ack).unwrap();
        let restored: InvitationAck = from_slice(&bytes).unwrap();

        assert_eq!(restored.invitation_id.as_str(), "inv-123");
        assert!(restored.success);
        assert_eq!(restored.status, "relationship_established");
    }

    #[test]
    fn test_guardian_decline_serialization() {
        let decline = GuardianDecline {
            invitation_id: InvitationId::new("guard-456"),
            reason: Some("Unable to commit".to_string()),
        };

        let bytes = to_vec(&decline).unwrap();
        let restored: GuardianDecline = from_slice(&bytes).unwrap();

        assert_eq!(restored.invitation_id.as_str(), "guard-456");
        assert_eq!(restored.reason, Some("Unable to commit".to_string()));
    }

    #[test]
    fn test_guardian_confirm_serialization() {
        let confirm = GuardianConfirm {
            invitation_id: InvitationId::new("guard-456"),
            established: true,
            relationship_id: Some("rel-789".to_string()),
        };

        let bytes = to_vec(&confirm).unwrap();
        let restored: GuardianConfirm = from_slice(&bytes).unwrap();

        assert_eq!(restored.invitation_id.as_str(), "guard-456");
        assert!(restored.established);
        assert_eq!(restored.relationship_id, Some("rel-789".to_string()));
    }

    #[test]
    fn test_device_enrollment_confirm_serialization() {
        let confirm = DeviceEnrollmentConfirm {
            invitation_id: InvitationId::new("enroll-123"),
            ceremony_id: CeremonyId::new("ceremony-456"),
            established: true,
            new_epoch: Some(5),
        };

        let bytes = to_vec(&confirm).unwrap();
        let restored: DeviceEnrollmentConfirm = from_slice(&bytes).unwrap();

        assert_eq!(restored.invitation_id.as_str(), "enroll-123");
        assert_eq!(restored.ceremony_id.as_str(), "ceremony-456");
        assert!(restored.established);
        assert_eq!(restored.new_epoch, Some(5));
    }
}
