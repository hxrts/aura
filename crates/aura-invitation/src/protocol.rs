//! Invitation Protocol Definitions
//!
//! MPST choreography definitions for invitation exchange and guardian invitation.
//! These define the message flow and guard annotations for invitation ceremonies.

use crate::facts::CeremonyRelationshipId;
use crate::InvitationType;
use aura_core::types::identifiers::{AuthorityId, CeremonyId, InvitationId};
use aura_core::{CapabilityName, DeviceId};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum InvitationAckStatus {
    Accepted,
    Declined,
}

impl InvitationAckStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            InvitationAckStatus::Accepted => "accepted",
            InvitationAckStatus::Declined => "declined",
        }
    }
}

impl fmt::Display for InvitationAckStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl FromStr for InvitationAckStatus {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "accepted" | "relationship_established" => Ok(Self::Accepted),
            "declined" | "declined_noted" => Ok(Self::Declined),
            _ => Err(format!("invalid invitation ack status: {value}")),
        }
    }
}

impl Serialize for InvitationAckStatus {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for InvitationAckStatus {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let raw = String::deserialize(deserializer)?;
        InvitationAckStatus::from_str(&raw).map_err(serde::de::Error::custom)
    }
}

/// Invitation acknowledgment message (confirms response received)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InvitationAck {
    /// Invitation identifier
    pub invitation_id: InvitationId,
    /// Whether the response was successfully processed
    pub success: bool,
    /// Result status
    pub status: InvitationAckStatus,
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
    pub recovery_capabilities: Vec<CapabilityName>,
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
    pub relationship_id: Option<CeremonyRelationshipId>,
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
    use aura_core::FlowCost;

    /// Flow cost for sending an invitation
    pub const INVITATION_SEND_COST: FlowCost = crate::guards::costs::INVITATION_SEND_COST;

    /// Flow cost for responding to an invitation
    pub const INVITATION_RESPOND_COST: FlowCost = crate::guards::costs::INVITATION_ACCEPT_COST;

    /// Flow cost for acknowledgment
    pub const INVITATION_ACK_COST: FlowCost = crate::guards::costs::INVITATION_ACCEPT_COST;

    /// Flow cost for guardian request
    pub const GUARDIAN_REQUEST_COST: FlowCost = FlowCost::new(2);

    /// Flow cost for guardian response
    pub const GUARDIAN_RESPOND_COST: FlowCost = FlowCost::new(2);

    /// Flow cost for guardian confirmation
    pub const GUARDIAN_CONFIRM_COST: FlowCost = FlowCost::new(1);

    /// Flow cost for device enrollment request
    pub const DEVICE_ENROLL_REQUEST_COST: FlowCost = FlowCost::new(2);

    /// Flow cost for device enrollment response
    pub const DEVICE_ENROLL_RESPOND_COST: FlowCost = FlowCost::new(2);

    /// Flow cost for device enrollment confirmation
    pub const DEVICE_ENROLL_CONFIRM_COST: FlowCost = FlowCost::new(1);
}

// =============================================================================
// Choreography Protocol Definitions
// =============================================================================

/// Basic invitation exchange protocol module
pub mod exchange {
    #![allow(unused_imports)]
    use super::*;
    use aura_macros::tell;

    // Invitation exchange choreography for basic invitation flow
    //
    // This choreography implements a simple invitation ceremony:
    // 1. Sender creates and sends invitation offer
    // 2. Receiver accepts or declines the invitation
    // 3. Sender acknowledges the response
    tell!(include_str!("src/protocol.invitation_exchange.tell"));
}

/// Guardian invitation protocol module
pub mod guardian {
    #![allow(unused_imports)]
    use super::*;
    use aura_macros::tell;

    // Guardian invitation choreography for establishing guardian relationships
    //
    // This choreography implements the guardian invitation ceremony:
    // 1. Principal requests guardian relationship
    // 2. Guardian accepts or declines with appropriate response
    // 3. Principal confirms the relationship establishment
    tell!(include_str!("src/protocol.guardian_invitation.tell"));
}

/// Device enrollment protocol module
pub mod device_enrollment {
    #![allow(unused_imports)]
    use super::*;
    use aura_macros::tell;

    // Device enrollment choreography for adding devices to an authority
    //
    // This choreography implements the device enrollment ceremony:
    // 1. Initiator (existing device) sends enrollment request with key package
    // 2. Invitee (new device) accepts and installs their share
    // 3. Initiator confirms the enrollment completion
    //
    // Note: The new device must create its own authority first, making it
    // addressable before the enrollment choreography can proceed.
    // The generated manifest carries device-migration link metadata for reconfiguration.
    // Runtime reconfiguration still consumes the device_migration bundle
    // contract exposed by this choreography surface.
    tell!(include_str!("src/protocol.device_enrollment.tell"));
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
    Failed { reason: InvitationProtocolFailure },
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
    Declined {
        reason: Option<InvitationDeclineReason>,
    },
    /// Relationship confirmed and established
    Confirmed {
        relationship_id: CeremonyRelationshipId,
    },
    /// Protocol failed
    Failed { reason: InvitationProtocolFailure },
}

/// State of the device enrollment protocol
#[derive(Debug, Clone)]
pub enum DeviceEnrollmentState {
    /// Initial state
    Initial,
    /// Enrollment request sent
    RequestSent,
    /// Enrollment accepted by invitee
    Accepted { device_id: DeviceId },
    /// Enrollment declined by invitee
    Declined {
        reason: Option<InvitationDeclineReason>,
    },
    /// Enrollment confirmed and established
    Confirmed { new_epoch: u64 },
    /// Protocol failed
    Failed { reason: InvitationProtocolFailure },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InvitationProtocolFailure {
    Timeout,
    GuardDenied,
    InvalidState { detail: String },
    Internal { detail: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InvitationDeclineReason {
    Provided { detail: String },
}

// =============================================================================
// Protocol Metadata
// =============================================================================

// =============================================================================
// Generated Runner Re-exports for execute_as Pattern
// =============================================================================

/// Re-exports for InvitationExchange choreography runners
pub mod exchange_runners {
    pub use super::exchange::telltale_session_types_invitation::invitation::runners::{
        execute_as, run_receiver, run_sender, ReceiverOutput, SenderOutput,
    };
    pub use super::exchange::telltale_session_types_invitation::invitation::InvitationExchangeRole;
}

/// Re-exports for GuardianInvitation choreography runners
pub mod guardian_runners {
    pub use super::guardian::telltale_session_types_invitation_guardian::invitation_guardian::GuardianInvitationRole;
    pub use super::guardian::telltale_session_types_invitation_guardian::invitation_guardian::runners::{
        execute_as, run_guardian, run_principal, GuardianOutput, PrincipalOutput,
    };
}

/// Re-exports for DeviceEnrollment choreography runners
pub mod device_enrollment_runners {
    pub use super::device_enrollment::telltale_session_types_invitation_device_enrollment::invitation_device_enrollment::DeviceEnrollmentRole;
    pub use super::device_enrollment::telltale_session_types_invitation_device_enrollment::invitation_device_enrollment::runners::{
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
    use aura_core::types::identifiers::{AuthorityId, CeremonyId, InvitationId};
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
    fn invitation_exchange_manifest_includes_protocol_metadata() {
        let manifest =
            exchange::telltale_session_types_invitation::vm_artifacts::composition_manifest();

        assert_eq!(manifest.protocol_name, "InvitationExchange");
        assert_eq!(manifest.protocol_namespace.as_deref(), Some("invitation"));
        assert_eq!(
            manifest.protocol_qualified_name,
            "invitation.InvitationExchange"
        );
        assert_eq!(manifest.protocol_id, "aura.invitation.exchange");
        assert_eq!(manifest.role_names, vec!["Sender", "Receiver"]);
        assert!(manifest.required_capabilities.is_empty());
        assert!(manifest.link_specs.is_empty());
        assert!(manifest.delegation_constraints.is_empty());
    }

    #[test]
    fn test_guardian_request_serialization() {
        let request = GuardianRequest {
            invitation_id: InvitationId::new("guard-456"),
            principal: test_authority(),
            role_description: "Primary guardian".to_string(),
            recovery_capabilities: vec![aura_core::capability_name!("recovery:initiate")],
            expires_at_ms: Some(2000000),
        };

        let bytes = to_vec(&request).unwrap();
        let restored: GuardianRequest = from_slice(&bytes).unwrap();

        assert_eq!(restored.invitation_id.as_str(), "guard-456");
        assert_eq!(restored.role_description, "Primary guardian");
        assert_eq!(
            restored.recovery_capabilities,
            vec![aura_core::capability_name!("recovery:initiate")]
        );
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
            relationship_id: CeremonyRelationshipId::parse("rel-0011223344556677")
                .unwrap_or_else(|error| panic!("valid relationship: {error}")),
        };
        if let GuardianInvitationState::Confirmed { relationship_id } = state {
            assert_eq!(relationship_id.as_str(), "rel-0011223344556677");
        }
    }

    #[test]
    fn test_protocol_failure_states_are_typed() {
        let exchange = InvitationExchangeState::Failed {
            reason: InvitationProtocolFailure::Timeout,
        };
        assert!(matches!(
            exchange,
            InvitationExchangeState::Failed {
                reason: InvitationProtocolFailure::Timeout
            }
        ));

        let guardian = GuardianInvitationState::Declined {
            reason: Some(InvitationDeclineReason::Provided {
                detail: "not available".to_string(),
            }),
        };
        assert!(matches!(
            guardian,
            GuardianInvitationState::Declined {
                reason: Some(InvitationDeclineReason::Provided { .. })
            }
        ));
    }

    #[test]
    fn test_guard_constants() {
        assert_eq!(guards::INVITATION_SEND_COST.value(), 1);
        assert_eq!(guards::GUARDIAN_REQUEST_COST.value(), 2);
        assert_eq!(
            crate::capabilities::InvitationCapability::Send
                .as_name()
                .as_str(),
            "invitation:send"
        );
        assert_eq!(
            crate::capabilities::InvitationCapability::Guardian
                .as_name()
                .as_str(),
            "invitation:guardian"
        );
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
            status: InvitationAckStatus::Accepted,
        };

        let bytes = to_vec(&ack).unwrap();
        let restored: InvitationAck = from_slice(&bytes).unwrap();

        assert_eq!(restored.invitation_id.as_str(), "inv-123");
        assert!(restored.success);
        assert_eq!(restored.status, InvitationAckStatus::Accepted);
    }

    #[test]
    fn test_invitation_ack_rejects_invalid_status() {
        let invalid = serde_json::json!({
            "invitation_id": "inv-123",
            "success": true,
            "status": "unknown-status"
        });
        let bytes = serde_json::to_vec(&invalid).unwrap();
        let restored = from_slice::<InvitationAck>(&bytes);
        assert!(restored.is_err());
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
            relationship_id: Some(
                CeremonyRelationshipId::parse("rel-0011223344556677")
                    .unwrap_or_else(|error| panic!("valid relationship id: {error}")),
            ),
        };

        let bytes = to_vec(&confirm).unwrap();
        let restored: GuardianConfirm = from_slice(&bytes).unwrap();

        assert_eq!(restored.invitation_id.as_str(), "guard-456");
        assert!(restored.established);
        assert_eq!(
            restored.relationship_id,
            Some(
                CeremonyRelationshipId::parse("rel-0011223344556677")
                    .unwrap_or_else(|error| panic!("valid relationship id: {error}"))
            )
        );
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
