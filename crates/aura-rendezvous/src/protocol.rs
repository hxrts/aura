//! Rendezvous Protocol Definitions
//!
//! MPST choreography definitions for rendezvous exchange and relayed rendezvous.
//! These define the message flow and guard annotations for peer discovery
//! and channel establishment.

// The choreography! macro generates unit returns which trigger this lint
#![allow(clippy::unused_unit)]

use aura_core::identifiers::AuthorityId;
use serde::{Deserialize, Serialize};

// =============================================================================
// Protocol Message Types
// =============================================================================

/// Noise IKpsk2 handshake message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NoiseHandshake {
    /// Noise protocol message bytes
    pub noise_message: Vec<u8>,
    /// Context-bound PSK commitment (hash of PSK)
    pub psk_commitment: [u8; 32],
    /// Epoch for key rotation synchronization
    pub epoch: u64,
}

/// Relay envelope for relayed rendezvous
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelayEnvelope {
    /// Encrypted inner envelope (contains actual message)
    pub ciphertext: Vec<u8>,
    /// Unlinkable routing tag for relay forwarding
    pub routing_tag: [u8; 32],
    /// Time-to-live hop count for relay chain
    pub ttl: u8,
}

/// Descriptor offer message (sent during exchange)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DescriptorOffer {
    /// The rendezvous descriptor being offered
    pub descriptor: crate::facts::RendezvousDescriptor,
}

/// Descriptor answer message (response to offer)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DescriptorAnswer {
    /// The responding peer's descriptor
    pub descriptor: crate::facts::RendezvousDescriptor,
}

/// Handshake initiation message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HandshakeInit {
    /// Noise handshake data
    pub handshake: NoiseHandshake,
}

/// Handshake completion message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HandshakeComplete {
    /// Noise handshake response data
    pub handshake: NoiseHandshake,
    /// Resulting channel identifier
    pub channel_id: [u8; 32],
}

/// Relay request message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelayRequest {
    /// Target peer for relay
    pub target: AuthorityId,
    /// Encrypted envelope to forward
    pub envelope: RelayEnvelope,
}

/// Relay forward message (relay to responder)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelayForward {
    /// Original sender (for response routing)
    pub sender: AuthorityId,
    /// Encrypted envelope
    pub envelope: RelayEnvelope,
}

/// Relay response message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelayResponse {
    /// Encrypted response envelope
    pub envelope: RelayEnvelope,
}

/// Relay completion message (back to initiator)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelayComplete {
    /// Encrypted response envelope
    pub envelope: RelayEnvelope,
}

// =============================================================================
// Guard Cost Constants
// =============================================================================

/// Guard annotations module for flow costs and capabilities
pub mod guards {
    use aura_core::FlowCost;

    /// Flow cost for publishing a descriptor
    pub const DESCRIPTOR_PUBLISH_COST: FlowCost = FlowCost::new(1);

    /// Flow cost for establishing a direct connection
    pub const CONNECT_DIRECT_COST: FlowCost = FlowCost::new(2);

    /// Flow cost for relay forwarding
    pub const RELAY_FORWARD_COST: FlowCost = FlowCost::new(1);

    /// Flow cost for relayed connection
    pub const RELAY_CONNECT_COST: FlowCost = FlowCost::new(2);

    /// Required capability for descriptor publication
    pub const CAP_RENDEZVOUS_PUBLISH: &str = "rendezvous:publish";

    /// Required capability for direct connection
    pub const CAP_RENDEZVOUS_CONNECT: &str = "rendezvous:connect";

    /// Required capability for relay usage
    pub const CAP_RENDEZVOUS_RELAY: &str = "rendezvous:relay";

    /// Required capability for relay forwarding (relay nodes)
    pub const CAP_RELAY_FORWARD: &str = "relay:forward";
}

// =============================================================================
// Choreography Protocol Definitions
// =============================================================================

/// Direct rendezvous exchange protocol module
pub mod exchange {
    #![allow(unused_imports)]
    use super::*;
    use aura_macros::choreography;

    // Rendezvous exchange choreography for direct peer discovery and channel establishment
    //
    // This choreography implements secure rendezvous between two authorities:
    // 1. Initiator publishes descriptor (fact insertion, propagates via sync)
    // 2. Responder publishes response descriptor
    // 3. Initiator initiates Noise IKpsk2 handshake
    // 4. Responder completes handshake, establishing secure channel
    choreography!(include_str!("src/protocol.rendezvous_exchange.choreo"));
}

/// Relayed rendezvous protocol module
pub mod relayed {
    #![allow(unused_imports)]
    use super::*;
    use aura_macros::choreography;

    // Relayed rendezvous choreography for relay-assisted connection
    //
    // This choreography implements relay-assisted rendezvous when direct connection fails:
    // 1. Initiator sends relay request to relay node
    // 2. Relay forwards to responder (with metadata leakage tracking)
    // 3. Responder sends response back through relay
    // 4. Relay completes by forwarding response to initiator
    choreography!(include_str!("src/protocol.relayed_rendezvous.choreo"));
}

// =============================================================================
// Protocol State Types
// =============================================================================

/// State of the rendezvous exchange protocol
#[derive(Debug, Clone)]
pub enum ExchangeState {
    /// Initial state - no descriptors exchanged
    Initial,
    /// Initiator has sent descriptor offer
    OfferSent,
    /// Responder has answered with descriptor
    AnswerReceived,
    /// Handshake initiated
    HandshakeStarted,
    /// Channel established
    Complete { channel_id: [u8; 32] },
    /// Protocol failed
    Failed { reason: String },
}

/// State of the relayed rendezvous protocol
#[derive(Debug, Clone)]
pub enum RelayedState {
    /// Initial state
    Initial,
    /// Relay request sent
    RequestSent,
    /// Relay forwarded to responder
    Forwarded,
    /// Response received from responder
    ResponseReceived,
    /// Relay complete - response delivered to initiator
    Complete,
    /// Protocol failed
    Failed { reason: String },
}

// =============================================================================
// Protocol Metadata
// =============================================================================

/// Protocol namespace for rendezvous
pub const PROTOCOL_NAMESPACE: &str = "rendezvous";

/// Protocol version
pub const PROTOCOL_VERSION: u32 = 1;

/// Protocol identifier for exchange
pub const EXCHANGE_PROTOCOL_ID: &str = "rendezvous.exchange.v1";

/// Protocol identifier for relayed exchange
pub const RELAYED_PROTOCOL_ID: &str = "rendezvous_relay.relayed.v1";

// =============================================================================
// Generated Runner Re-exports for execute_as Pattern
// =============================================================================

/// Re-exports for RendezvousExchange choreography runners
pub mod exchange_runners {
    pub use super::exchange::rumpsteak_session_types_rendezvous::rendezvous::runners::{
        execute_as, run_initiator, run_responder, InitiatorOutput, ResponderOutput,
    };
    pub use super::exchange::rumpsteak_session_types_rendezvous::rendezvous::RendezvousExchangeRole;
}

/// Re-exports for RelayedRendezvous choreography runners
pub mod relayed_runners {
    pub use super::relayed::rumpsteak_session_types_rendezvous_relay::rendezvous_relay::runners::{
        execute_as, run_initiator, run_relay, run_responder, InitiatorOutput, RelayOutput,
        ResponderOutput,
    };
    pub use super::relayed::rumpsteak_session_types_rendezvous_relay::rendezvous_relay::RelayedRendezvousRole;
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_noise_handshake_serialization() {
        let handshake = NoiseHandshake {
            noise_message: vec![1, 2, 3, 4],
            psk_commitment: [42u8; 32],
            epoch: 5,
        };

        let bytes = serde_json::to_vec(&handshake).unwrap();
        let restored: NoiseHandshake = serde_json::from_slice(&bytes).unwrap();

        assert_eq!(restored.noise_message, vec![1, 2, 3, 4]);
        assert_eq!(restored.psk_commitment, [42u8; 32]);
        assert_eq!(restored.epoch, 5);
    }

    #[test]
    fn test_relay_envelope_serialization() {
        let envelope = RelayEnvelope {
            ciphertext: vec![10, 20, 30],
            routing_tag: [99u8; 32],
            ttl: 3,
        };

        let bytes = serde_json::to_vec(&envelope).unwrap();
        let restored: RelayEnvelope = serde_json::from_slice(&bytes).unwrap();

        assert_eq!(restored.ciphertext, vec![10, 20, 30]);
        assert_eq!(restored.routing_tag, [99u8; 32]);
        assert_eq!(restored.ttl, 3);
    }

    #[test]
    #[allow(unused_assignments)]
    fn test_exchange_state_transitions() {
        let mut state = ExchangeState::Initial;

        // Simulate state transitions
        state = ExchangeState::OfferSent;
        assert!(matches!(state, ExchangeState::OfferSent));

        state = ExchangeState::AnswerReceived;
        assert!(matches!(state, ExchangeState::AnswerReceived));

        state = ExchangeState::HandshakeStarted;
        assert!(matches!(state, ExchangeState::HandshakeStarted));

        state = ExchangeState::Complete {
            channel_id: [1u8; 32],
        };
        if let ExchangeState::Complete { channel_id } = state {
            assert_eq!(channel_id, [1u8; 32]);
        } else {
            panic!("Expected Complete state");
        }
    }

    #[test]
    fn test_guard_constants() {
        assert_eq!(guards::DESCRIPTOR_PUBLISH_COST.value(), 1);
        assert_eq!(guards::CONNECT_DIRECT_COST.value(), 2);
        assert_eq!(guards::RELAY_FORWARD_COST.value(), 1);
        assert_eq!(guards::CAP_RENDEZVOUS_PUBLISH, "rendezvous:publish");
    }

    #[test]
    fn test_protocol_metadata() {
        assert_eq!(PROTOCOL_NAMESPACE, "rendezvous");
        assert_eq!(PROTOCOL_VERSION, 1);
        assert!(EXCHANGE_PROTOCOL_ID.contains("rendezvous"));
        assert!(RELAYED_PROTOCOL_ID.contains("relayed"));
    }

    #[test]
    fn test_descriptor_offer_serialization() {
        use crate::facts::{RendezvousDescriptor, TransportHint};
        use aura_core::identifiers::{AuthorityId, ContextId};

        let descriptor = RendezvousDescriptor {
            authority_id: AuthorityId::new_from_entropy([1u8; 32]),
            context_id: ContextId::new_from_entropy([2u8; 32]),
            transport_hints: vec![TransportHint::tcp_direct("127.0.0.1:8080").unwrap()],
            handshake_psk_commitment: [0u8; 32],
            valid_from: 1000,
            valid_until: 2000,
            nonce: [3u8; 32],
            nickname_suggestion: None,
        };

        let offer = DescriptorOffer { descriptor };
        let bytes = serde_json::to_vec(&offer).unwrap();
        let restored: DescriptorOffer = serde_json::from_slice(&bytes).unwrap();

        assert_eq!(restored.descriptor.valid_from, 1000);
    }

    #[test]
    fn test_handshake_init_serialization() {
        let init = HandshakeInit {
            handshake: NoiseHandshake {
                noise_message: vec![1, 2, 3],
                psk_commitment: [4u8; 32],
                epoch: 1,
            },
        };

        let bytes = serde_json::to_vec(&init).unwrap();
        let restored: HandshakeInit = serde_json::from_slice(&bytes).unwrap();

        assert_eq!(restored.handshake.epoch, 1);
    }

    #[test]
    fn test_handshake_complete_serialization() {
        let complete = HandshakeComplete {
            handshake: NoiseHandshake {
                noise_message: vec![5, 6, 7],
                psk_commitment: [8u8; 32],
                epoch: 2,
            },
            channel_id: [9u8; 32],
        };

        let bytes = serde_json::to_vec(&complete).unwrap();
        let restored: HandshakeComplete = serde_json::from_slice(&bytes).unwrap();

        assert_eq!(restored.channel_id, [9u8; 32]);
    }

    #[test]
    fn test_relay_request_serialization() {
        use aura_core::identifiers::AuthorityId;

        let request = RelayRequest {
            target: AuthorityId::new_from_entropy([10u8; 32]),
            envelope: RelayEnvelope {
                ciphertext: vec![1, 2, 3],
                routing_tag: [11u8; 32],
                ttl: 2,
            },
        };

        let bytes = serde_json::to_vec(&request).unwrap();
        let restored: RelayRequest = serde_json::from_slice(&bytes).unwrap();

        assert_eq!(restored.envelope.ttl, 2);
    }

    #[test]
    #[allow(unused_assignments)]
    fn test_relayed_state_transitions() {
        let mut state = RelayedState::Initial;

        state = RelayedState::RequestSent;
        assert!(matches!(state, RelayedState::RequestSent));

        state = RelayedState::Forwarded;
        assert!(matches!(state, RelayedState::Forwarded));

        state = RelayedState::ResponseReceived;
        assert!(matches!(state, RelayedState::ResponseReceived));

        state = RelayedState::Complete;
        assert!(matches!(state, RelayedState::Complete));
    }

    #[test]
    fn test_relayed_state_failure() {
        let state = RelayedState::Failed {
            reason: "connection timeout".to_string(),
        };

        if let RelayedState::Failed { reason } = state {
            assert_eq!(reason, "connection timeout");
        } else {
            panic!("Expected Failed state");
        }
    }
}
