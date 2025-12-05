//! Invitation Protocol Definitions
//!
//! MPST choreography definitions for invitation exchange and guardian invitation.
//! These define the message flow and guard annotations for invitation ceremonies.

use aura_core::identifiers::AuthorityId;
use serde::{Deserialize, Serialize};

// =============================================================================
// Protocol Message Types
// =============================================================================

/// Invitation offer message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InvitationOffer {
    /// Unique invitation identifier
    pub invitation_id: String,
    /// Type of invitation (device, guardian, channel, etc.)
    pub invitation_type: String,
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
    pub invitation_id: String,
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
    pub invitation_id: String,
    /// Whether the response was successfully processed
    pub success: bool,
    /// Result status (e.g., "relationship_established", "declined_noted")
    pub status: String,
}

/// Guardian invitation request (specialized for guardian relationships)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuardianRequest {
    /// Unique invitation identifier
    pub invitation_id: String,
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
    pub invitation_id: String,
    /// Guardian's acceptance signature
    pub signature: Vec<u8>,
    /// Guardian's public key for recovery operations
    pub recovery_public_key: Vec<u8>,
}

/// Guardian decline response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuardianDecline {
    /// Invitation identifier
    pub invitation_id: String,
    /// Optional reason for declining
    pub reason: Option<String>,
}

/// Guardian confirmation (finalizes relationship)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuardianConfirm {
    /// Invitation identifier
    pub invitation_id: String,
    /// Relationship established successfully
    pub established: bool,
    /// Resulting relationship identifier
    pub relationship_id: Option<String>,
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
    choreography! {
        #[namespace = "invitation"]
        protocol InvitationExchange {
            roles: Sender, Receiver;

            // Sender offers invitation
            Sender[guard_capability = "invitation:send",
                   flow_cost = 1,
                   journal_facts = "InvitationFact::Sent"]
            -> Receiver: InvitationOffer(super::InvitationOffer);

            // Receiver responds (accept or decline)
            Receiver[guard_capability = "invitation:accept",
                     flow_cost = 1,
                     journal_facts = "InvitationFact::Responded"]
            -> Sender: InvitationResponse(super::InvitationResponse);

            // Sender acknowledges response
            Sender[guard_capability = "invitation:send",
                   flow_cost = 1,
                   journal_facts = "InvitationFact::Acknowledged"]
            -> Receiver: InvitationAck(super::InvitationAck);
        }
    }
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
    choreography! {
        #[namespace = "invitation_guardian"]
        protocol GuardianInvitation {
            roles: Principal, Guardian;

            // Principal sends guardian request
            Principal[guard_capability = "invitation:guardian",
                      flow_cost = 2,
                      journal_facts = "InvitationFact::GuardianRequested"]
            -> Guardian: GuardianRequest(super::GuardianRequest);

            // Guardian accepts (or use separate decline path)
            Guardian[guard_capability = "invitation:guardian:accept",
                     flow_cost = 2,
                     journal_facts = "InvitationFact::GuardianAccepted"]
            -> Principal: GuardianAccept(super::GuardianAccept);

            // Principal confirms relationship
            Principal[guard_capability = "invitation:guardian",
                      flow_cost = 1,
                      journal_facts = "InvitationFact::GuardianConfirmed"]
            -> Guardian: GuardianConfirm(super::GuardianConfirm);
        }
    }
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

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::identifiers::AuthorityId;

    fn test_authority() -> AuthorityId {
        AuthorityId::new_from_entropy([1u8; 32])
    }

    #[test]
    fn test_invitation_offer_serialization() {
        let offer = InvitationOffer {
            invitation_id: "inv-123".to_string(),
            invitation_type: "device".to_string(),
            sender: test_authority(),
            message: Some("Please join".to_string()),
            expires_at_ms: Some(1000000),
            commitment: [42u8; 32],
        };

        let bytes = serde_json::to_vec(&offer).unwrap();
        let restored: InvitationOffer = serde_json::from_slice(&bytes).unwrap();

        assert_eq!(restored.invitation_id, "inv-123");
        assert_eq!(restored.invitation_type, "device");
        assert_eq!(restored.message, Some("Please join".to_string()));
    }

    #[test]
    fn test_invitation_response_serialization() {
        let response = InvitationResponse {
            invitation_id: "inv-123".to_string(),
            accepted: true,
            message: Some("Happy to join".to_string()),
            signature: vec![1, 2, 3, 4],
        };

        let bytes = serde_json::to_vec(&response).unwrap();
        let restored: InvitationResponse = serde_json::from_slice(&bytes).unwrap();

        assert_eq!(restored.invitation_id, "inv-123");
        assert!(restored.accepted);
    }

    #[test]
    fn test_guardian_request_serialization() {
        let request = GuardianRequest {
            invitation_id: "guard-456".to_string(),
            principal: test_authority(),
            role_description: "Primary guardian".to_string(),
            recovery_capabilities: vec!["recover:device".to_string()],
            expires_at_ms: Some(2000000),
        };

        let bytes = serde_json::to_vec(&request).unwrap();
        let restored: GuardianRequest = serde_json::from_slice(&bytes).unwrap();

        assert_eq!(restored.invitation_id, "guard-456");
        assert_eq!(restored.role_description, "Primary guardian");
    }

    #[test]
    fn test_guardian_accept_serialization() {
        let accept = GuardianAccept {
            invitation_id: "guard-456".to_string(),
            signature: vec![5, 6, 7, 8],
            recovery_public_key: vec![9, 10, 11, 12],
        };

        let bytes = serde_json::to_vec(&accept).unwrap();
        let restored: GuardianAccept = serde_json::from_slice(&bytes).unwrap();

        assert_eq!(restored.invitation_id, "guard-456");
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
            invitation_id: "inv-123".to_string(),
            success: true,
            status: "relationship_established".to_string(),
        };

        let bytes = serde_json::to_vec(&ack).unwrap();
        let restored: InvitationAck = serde_json::from_slice(&bytes).unwrap();

        assert_eq!(restored.invitation_id, "inv-123");
        assert!(restored.success);
        assert_eq!(restored.status, "relationship_established");
    }

    #[test]
    fn test_guardian_decline_serialization() {
        let decline = GuardianDecline {
            invitation_id: "guard-456".to_string(),
            reason: Some("Unable to commit".to_string()),
        };

        let bytes = serde_json::to_vec(&decline).unwrap();
        let restored: GuardianDecline = serde_json::from_slice(&bytes).unwrap();

        assert_eq!(restored.invitation_id, "guard-456");
        assert_eq!(restored.reason, Some("Unable to commit".to_string()));
    }

    #[test]
    fn test_guardian_confirm_serialization() {
        let confirm = GuardianConfirm {
            invitation_id: "guard-456".to_string(),
            established: true,
            relationship_id: Some("rel-789".to_string()),
        };

        let bytes = serde_json::to_vec(&confirm).unwrap();
        let restored: GuardianConfirm = serde_json::from_slice(&bytes).unwrap();

        assert_eq!(restored.invitation_id, "guard-456");
        assert!(restored.established);
        assert_eq!(restored.relationship_id, Some("rel-789".to_string()));
    }
}
