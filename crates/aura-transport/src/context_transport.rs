//! Context-Aware Transport System
//!
//! This module provides transport types that align with the
//! authority-centric model and uses ContextId for scoping.
//!
//! As a Layer 2 (Specification) module, this only defines types.
//! Actual coordination logic belongs in Layer 4 (aura-protocol).

use crate::types::endpoint::EndpointAddress;
use aura_core::identifiers::ContextId;
use aura_core::AuthorityId;
use serde::{Deserialize, Serialize};

/// Context-scoped transport session
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextTransportSession {
    /// Session ID
    pub session_id: String,
    /// Context this session belongs to
    pub context_id: ContextId,
    /// Local authority
    pub local_authority: AuthorityId,
    /// Remote authority
    pub remote_authority: AuthorityId,
    /// Transport protocol in use
    pub protocol: TransportProtocol,
    /// Session state
    pub state: SessionState,
    /// Authorization level for this session (actual authorization via Biscuit tokens in protocol layer)
    pub authorization_level: String,
    /// Flow budget remaining
    pub flow_budget: i64,
}

#[allow(missing_docs)]
impl ContextTransportSession {}

/// Transport protocol types
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TransportProtocol {
    /// QUIC transport
    Quic {
        /// Socket address for QUIC endpoint
        endpoint: EndpointAddress,
    },
    /// TCP transport
    Tcp {
        /// Socket address for TCP endpoint
        endpoint: EndpointAddress,
    },
    /// WebRTC transport
    WebRTC {
        /// Peer identifier for WebRTC connection
        peer_id: String,
    },
    /// Relay transport via another authority
    Relay {
        /// Authority to relay through
        relay_authority: AuthorityId,
    },
}

/// Session state
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SessionState {
    /// Session being established
    Connecting,
    /// Session active
    Active,
    /// Session closing
    Closing,
    /// Session closed
    Closed,
}

/// Context transport configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextTransportConfig {
    /// Default session timeout
    pub session_timeout: std::time::Duration,
    /// Maximum concurrent sessions per context
    pub max_sessions_per_context: u32,
    /// Flow budget per session
    pub default_flow_budget: i64,
    /// Supported protocols
    pub supported_protocols: Vec<TransportProtocol>,
}

impl Default for ContextTransportConfig {
    fn default() -> Self {
        Self {
            session_timeout: std::time::Duration::from_secs(300),
            max_sessions_per_context: 10,
            default_flow_budget: 10000,
            supported_protocols: vec![
                TransportProtocol::Quic {
                    endpoint: EndpointAddress::new("[::]:0"),
                },
                TransportProtocol::Tcp {
                    endpoint: EndpointAddress::new("[::]:0"),
                },
            ],
        }
    }
}

/// Context-aware transport endpoint
pub struct ContextTransportEndpoint {
    /// Authority ID
    pub authority_id: AuthorityId,
    /// Available protocols
    pub protocols: Vec<TransportProtocol>,
    /// Supported contexts
    pub contexts: Vec<ContextId>,
}

/// Context transport message types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ContextTransportMessage {
    /// Session establishment request
    SessionRequest {
        /// Context ID for the session
        context_id: ContextId,
        /// Authority requesting the session
        authority_id: AuthorityId,
        /// Supported transport protocols
        protocols: Vec<TransportProtocol>,
        /// Authorization level requested
        authorization_level: String,
    },
    /// Session establishment response
    SessionResponse {
        /// Identifier for the established session
        session_id: String,
        /// Selected transport protocol
        selected_protocol: TransportProtocol,
        /// Initial flow budget granted
        flow_budget: i64,
    },
    /// Data message
    Data {
        /// Session identifier
        session_id: String,
        /// Sequence number for ordering
        sequence: u64,
        /// Message payload
        payload: Vec<u8>,
    },
    /// Session control
    Control {
        /// Session identifier
        session_id: String,
        /// Control command
        command: SessionControl,
    },
}

/// Session control commands
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SessionControl {
    /// Keep-alive ping
    Ping,
    /// Keep-alive pong
    Pong,
    /// Request flow budget increase
    RequestBudget(i64),
    /// Grant flow budget increase
    GrantBudget(i64),
    /// Close session
    Close,
}

/// Transport metrics for a context
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ContextTransportMetrics {
    /// Total sessions created
    pub sessions_created: u64,
    /// Currently active sessions
    pub active_sessions: u64,
    /// Total bytes sent
    pub bytes_sent: u64,
    /// Total bytes received
    pub bytes_received: u64,
    /// Flow budget consumed
    pub flow_budget_consumed: i64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_transport_protocol_serialization() {
        let protocol = TransportProtocol::Quic {
            endpoint: EndpointAddress::new("127.0.0.1:8080"),
        };

        let serialized = serde_json::to_string(&protocol).unwrap();
        let deserialized: TransportProtocol = serde_json::from_str(&serialized).unwrap();

        assert_eq!(protocol, deserialized);
    }

    #[test]
    fn test_session_state_transitions() {
        let state = SessionState::Connecting;
        assert_ne!(state, SessionState::Active);
    }

    #[test]
    fn test_context_transport_config_default() {
        let config = ContextTransportConfig::default();
        assert_eq!(config.max_sessions_per_context, 10);
        assert_eq!(config.default_flow_budget, 10000);
    }
}
