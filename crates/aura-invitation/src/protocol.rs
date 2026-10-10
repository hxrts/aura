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
    /// The inviter key that signed the imported invitation code. The
    /// principal must confirm with the matching retained epoch key.
    pub invitation_sender_proof_key: Vec<u8>,
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
    /// Principal signature created only after verifying the guardian response.
    pub signature: Vec<u8>,
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
    /// Authority that accepted (the invitation receiver).
    pub acceptor_id: AuthorityId,
    /// Acceptor signature over the device-enrollment acceptance transcript.
    pub signature: aura_core::threshold::ThresholdSignature,
    /// Digest of the actual independently admitted signed enrollment manifest.
    /// Required binding; an absent digest is not a current response encoding.
    pub manifest_digest: [u8; 32],
}

/// A response is accepted or refused under separate signing domains.
/// The payload shape never proves either disposition without verification.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum DeviceEnrollmentResponse {
    Accepted(DeviceEnrollmentAccept),
    Refused(DeviceEnrollmentRefusal),
}

/// Refusal shares the same complete ceremony binding as acceptance, but its
/// signature covers the dedicated refusal transcript and cannot count a device.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeviceEnrollmentRefusal {
    pub binding: DeviceEnrollmentAccept,
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

/// A received enrollment message does not establish its expected postcondition.
/// These checks bind message contents; authentication is required separately.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum DeviceEnrollmentMessageError {
    /// The message belongs to another invitation.
    #[error("device enrollment invitation mismatch")]
    InvitationMismatch,
    /// The request modifies another authority.
    #[error("device enrollment subject authority mismatch")]
    SubjectMismatch,
    /// The message belongs to another ceremony.
    #[error("device enrollment ceremony mismatch")]
    CeremonyMismatch,
    /// The request enrolls another device.
    #[error("device enrollment device mismatch")]
    DeviceMismatch,
    /// The message carries another or missing epoch.
    #[error("device enrollment epoch mismatch")]
    EpochMismatch,
    /// The principal did not establish enrollment.
    #[error("device enrollment was not established")]
    NotEstablished,
}

impl DeviceEnrollmentRequest {
    /// Require the request to match the invitation-owned expected request.
    pub fn validate_against(&self, expected: &Self) -> Result<(), DeviceEnrollmentMessageError> {
        if self.invitation_id != expected.invitation_id {
            return Err(DeviceEnrollmentMessageError::InvitationMismatch);
        }
        if self.subject_authority != expected.subject_authority {
            return Err(DeviceEnrollmentMessageError::SubjectMismatch);
        }
        if self.ceremony_id != expected.ceremony_id {
            return Err(DeviceEnrollmentMessageError::CeremonyMismatch);
        }
        if self.device_id != expected.device_id {
            return Err(DeviceEnrollmentMessageError::DeviceMismatch);
        }
        if self.pending_epoch != expected.pending_epoch {
            return Err(DeviceEnrollmentMessageError::EpochMismatch);
        }
        Ok(())
    }
}

impl DeviceEnrollmentConfirm {
    /// Require successful confirmation of the exact matched request.
    pub fn validate_against(
        &self,
        request: &DeviceEnrollmentRequest,
    ) -> Result<(), DeviceEnrollmentMessageError> {
        if self.invitation_id != request.invitation_id {
            return Err(DeviceEnrollmentMessageError::InvitationMismatch);
        }
        if self.ceremony_id != request.ceremony_id {
            return Err(DeviceEnrollmentMessageError::CeremonyMismatch);
        }
        if !self.established {
            return Err(DeviceEnrollmentMessageError::NotEstablished);
        }
        if self.new_epoch != Some(request.pending_epoch) {
            return Err(DeviceEnrollmentMessageError::EpochMismatch);
        }
        Ok(())
    }
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

/// Untrusted envelope for the separately admitted negative terminal channel.
/// Only the signed runtime control inside it can issue a failure capability.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeviceEnrollmentTerminalNotice {
    pub invitation_id: InvitationId,
    pub ceremony_id: CeremonyId,
    pub signed_control: Vec<u8>,
}
impl DeviceEnrollmentTerminalNotice {
    pub const MAX_WIRE_BYTES: usize = 16_384;
}

/// The negative notice never advances the enrollment response VM.
pub mod device_enrollment_terminal_notice {
    #![allow(unused_imports)]
    use super::*;
    use aura_macros::tell;
    tell!(include_str!(
        "src/protocol.device_enrollment_terminal_notice.tell"
    ));
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
    fn device_enrollment_messages_bind_every_invitation_dimension() {
        use DeviceEnrollmentMessageError::*;
        let request = DeviceEnrollmentRequest {
            invitation_id: InvitationId::new("enrollment-message-binding"),
            subject_authority: test_authority(),
            ceremony_id: CeremonyId::new("enrollment-message-ceremony"),
            pending_epoch: 7,
            device_id: DeviceId::new_from_entropy([2; 32]),
        };
        assert_eq!(request.validate_against(&request), Ok(()));
        for (changed, error) in [
            (
                DeviceEnrollmentRequest {
                    invitation_id: InvitationId::new("other"),
                    ..request.clone()
                },
                InvitationMismatch,
            ),
            (
                DeviceEnrollmentRequest {
                    subject_authority: AuthorityId::new_from_entropy([3; 32]),
                    ..request.clone()
                },
                SubjectMismatch,
            ),
            (
                DeviceEnrollmentRequest {
                    ceremony_id: CeremonyId::new("other"),
                    ..request.clone()
                },
                CeremonyMismatch,
            ),
            (
                DeviceEnrollmentRequest {
                    device_id: DeviceId::new_from_entropy([4; 32]),
                    ..request.clone()
                },
                DeviceMismatch,
            ),
            (
                DeviceEnrollmentRequest {
                    pending_epoch: 8,
                    ..request.clone()
                },
                EpochMismatch,
            ),
        ] {
            assert_eq!(changed.validate_against(&request), Err(error));
        }

        let confirmation = DeviceEnrollmentConfirm {
            invitation_id: request.invitation_id.clone(),
            ceremony_id: request.ceremony_id.clone(),
            established: true,
            new_epoch: Some(request.pending_epoch),
        };
        assert_eq!(confirmation.validate_against(&request), Ok(()));
        for (changed, error) in [
            (
                DeviceEnrollmentConfirm {
                    invitation_id: InvitationId::new("other"),
                    ..confirmation.clone()
                },
                InvitationMismatch,
            ),
            (
                DeviceEnrollmentConfirm {
                    ceremony_id: CeremonyId::new("other"),
                    ..confirmation.clone()
                },
                CeremonyMismatch,
            ),
            (
                DeviceEnrollmentConfirm {
                    established: false,
                    ..confirmation.clone()
                },
                NotEstablished,
            ),
            (
                DeviceEnrollmentConfirm {
                    new_epoch: None,
                    ..confirmation.clone()
                },
                EpochMismatch,
            ),
            (
                DeviceEnrollmentConfirm {
                    new_epoch: Some(8),
                    ..confirmation
                },
                EpochMismatch,
            ),
        ] {
            assert_eq!(changed.validate_against(&request), Err(error));
        }
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
            invitation_sender_proof_key: vec![13; 32],
        };

        let bytes = to_vec(&accept).unwrap();
        let restored: GuardianAccept = from_slice(&bytes).unwrap();

        assert_eq!(restored.invitation_id.as_str(), "guard-456");
        assert_eq!(restored.recovery_public_key, vec![9, 10, 11, 12]);
        assert_eq!(restored.invitation_sender_proof_key, vec![13; 32]);
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
            signature: vec![1, 2, 3],
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
