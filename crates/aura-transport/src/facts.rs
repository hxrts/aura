//! Transport Domain Facts
//!
//! Pure fact types for transport layer state changes.
//! These facts are defined here (Layer 2) and committed by higher layers.

use aura_core::identifiers::{AuthorityId, ContextId};
use aura_core::time::PhysicalTime;
use aura_core::types::facts::{FactDelta, FactDeltaReducer};
use aura_core::util::serialization::{from_slice, to_vec, SemanticVersion, VersionedMessage};
use serde::{Deserialize, Serialize};

use crate::context_transport::TransportProtocol;

/// Unique type identifier for transport facts
pub const TRANSPORT_FACT_TYPE_ID: &str = "transport/v1";
/// Schema version for transport fact encoding
pub const TRANSPORT_FACT_SCHEMA_VERSION: u16 = 1;

/// Transport domain facts for state changes.
///
/// These facts capture transport layer events and are used by the
/// journal system to derive transport state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TransportFact {
    /// Session established between two authorities
    SessionEstablished {
        /// Unique session identifier
        session_id: String,
        /// Context for the session
        context_id: ContextId,
        /// Local authority
        local_authority: AuthorityId,
        /// Remote authority
        remote_authority: AuthorityId,
        /// Protocol used
        protocol: TransportProtocol,
        /// Timestamp when session was established (uses unified time system)
        established_at: PhysicalTime,
    },

    /// Session closed
    SessionClosed {
        /// Session that was closed
        session_id: String,
        /// Context of the session
        context_id: ContextId,
        /// Reason for closure
        reason: String,
        /// Timestamp when session was closed (uses unified time system)
        closed_at: PhysicalTime,
    },

    /// Message sent through transport
    MessageSent {
        /// Context of the message
        context_id: ContextId,
        /// Session used for sending
        session_id: String,
        /// Hash of the message content
        message_hash: [u8; 32],
        /// Size of the message in bytes
        size_bytes: u64,
        /// Flow budget cost charged
        flow_cost: u32,
        /// Timestamp when message was sent (uses unified time system)
        sent_at: PhysicalTime,
    },

    /// Message received through transport
    MessageReceived {
        /// Context of the message
        context_id: ContextId,
        /// Session used for receiving
        session_id: String,
        /// Hash of the message content
        message_hash: [u8; 32],
        /// Size of the message in bytes
        size_bytes: u64,
        /// Sender authority
        sender: AuthorityId,
        /// Timestamp when message was received (uses unified time system)
        received_at: PhysicalTime,
    },

    /// Peer discovered through rendezvous or other mechanism
    PeerDiscovered {
        /// Context in which peer was discovered
        context_id: ContextId,
        /// Authority of the discovered peer
        authority_id: AuthorityId,
        /// Available transport protocols
        protocols: Vec<TransportProtocol>,
        /// Timestamp when peer was discovered (uses unified time system)
        discovered_at: PhysicalTime,
    },

    /// Peer connection failed
    ConnectionFailed {
        /// Context of the attempted connection
        context_id: ContextId,
        /// Target authority
        target_authority: AuthorityId,
        /// Reason for failure
        reason: String,
        /// Timestamp when connection failed (uses unified time system)
        failed_at: PhysicalTime,
    },

    /// Flow budget charged for transport operation
    FlowBudgetCharged {
        /// Context for the charge
        context_id: ContextId,
        /// Authority being charged
        authority_id: AuthorityId,
        /// Amount charged
        amount: u32,
        /// Current spent counter after charge
        spent_after: u64,
        /// Timestamp when charge occurred (uses unified time system)
        charged_at: PhysicalTime,
    },

    /// Hole punch attempt completed
    HolePunchCompleted {
        /// Session ID for the hole punch
        session_id: String,
        /// Context in which hole punch occurred
        context_id: ContextId,
        /// Local authority
        local_authority: AuthorityId,
        /// Remote authority
        remote_authority: AuthorityId,
        /// Whether hole punch was successful
        success: bool,
        /// Timestamp when hole punch completed (uses unified time system)
        completed_at: PhysicalTime,
    },
}

impl TransportFact {
    fn version() -> SemanticVersion {
        SemanticVersion::new(TRANSPORT_FACT_SCHEMA_VERSION, 0, 0)
    }

    /// Get the context ID for this fact
    pub fn context_id(&self) -> ContextId {
        match self {
            TransportFact::SessionEstablished { context_id, .. } => *context_id,
            TransportFact::SessionClosed { context_id, .. } => *context_id,
            TransportFact::MessageSent { context_id, .. } => *context_id,
            TransportFact::MessageReceived { context_id, .. } => *context_id,
            TransportFact::PeerDiscovered { context_id, .. } => *context_id,
            TransportFact::ConnectionFailed { context_id, .. } => *context_id,
            TransportFact::FlowBudgetCharged { context_id, .. } => *context_id,
            TransportFact::HolePunchCompleted { context_id, .. } => *context_id,
        }
    }

    /// Get the timestamp for this fact in milliseconds (backward compatibility)
    pub fn timestamp_ms(&self) -> u64 {
        match self {
            TransportFact::SessionEstablished { established_at, .. } => established_at.ts_ms,
            TransportFact::SessionClosed { closed_at, .. } => closed_at.ts_ms,
            TransportFact::MessageSent { sent_at, .. } => sent_at.ts_ms,
            TransportFact::MessageReceived { received_at, .. } => received_at.ts_ms,
            TransportFact::PeerDiscovered { discovered_at, .. } => discovered_at.ts_ms,
            TransportFact::ConnectionFailed { failed_at, .. } => failed_at.ts_ms,
            TransportFact::FlowBudgetCharged { charged_at, .. } => charged_at.ts_ms,
            TransportFact::HolePunchCompleted { completed_at, .. } => completed_at.ts_ms,
        }
    }

    /// Create a SessionEstablished fact with millisecond timestamp (backward compatibility)
    pub fn session_established_ms(
        session_id: String,
        context_id: ContextId,
        local_authority: AuthorityId,
        remote_authority: AuthorityId,
        protocol: TransportProtocol,
        established_at_ms: u64,
    ) -> Self {
        Self::SessionEstablished {
            session_id,
            context_id,
            local_authority,
            remote_authority,
            protocol,
            established_at: PhysicalTime {
                ts_ms: established_at_ms,
                uncertainty: None,
            },
        }
    }

    /// Create a SessionClosed fact with millisecond timestamp (backward compatibility)
    pub fn session_closed_ms(
        session_id: String,
        context_id: ContextId,
        reason: String,
        closed_at_ms: u64,
    ) -> Self {
        Self::SessionClosed {
            session_id,
            context_id,
            reason,
            closed_at: PhysicalTime {
                ts_ms: closed_at_ms,
                uncertainty: None,
            },
        }
    }

    /// Create a MessageSent fact with millisecond timestamp (backward compatibility)
    pub fn message_sent_ms(
        context_id: ContextId,
        session_id: String,
        message_hash: [u8; 32],
        size_bytes: u64,
        flow_cost: u32,
        sent_at_ms: u64,
    ) -> Self {
        Self::MessageSent {
            context_id,
            session_id,
            message_hash,
            size_bytes,
            flow_cost,
            sent_at: PhysicalTime {
                ts_ms: sent_at_ms,
                uncertainty: None,
            },
        }
    }

    /// Create a MessageReceived fact with millisecond timestamp (backward compatibility)
    pub fn message_received_ms(
        context_id: ContextId,
        session_id: String,
        message_hash: [u8; 32],
        size_bytes: u64,
        sender: AuthorityId,
        received_at_ms: u64,
    ) -> Self {
        Self::MessageReceived {
            context_id,
            session_id,
            message_hash,
            size_bytes,
            sender,
            received_at: PhysicalTime {
                ts_ms: received_at_ms,
                uncertainty: None,
            },
        }
    }

    /// Create a PeerDiscovered fact with millisecond timestamp (backward compatibility)
    pub fn peer_discovered_ms(
        context_id: ContextId,
        authority_id: AuthorityId,
        protocols: Vec<TransportProtocol>,
        discovered_at_ms: u64,
    ) -> Self {
        Self::PeerDiscovered {
            context_id,
            authority_id,
            protocols,
            discovered_at: PhysicalTime {
                ts_ms: discovered_at_ms,
                uncertainty: None,
            },
        }
    }

    /// Create a ConnectionFailed fact with millisecond timestamp (backward compatibility)
    pub fn connection_failed_ms(
        context_id: ContextId,
        target_authority: AuthorityId,
        reason: String,
        failed_at_ms: u64,
    ) -> Self {
        Self::ConnectionFailed {
            context_id,
            target_authority,
            reason,
            failed_at: PhysicalTime {
                ts_ms: failed_at_ms,
                uncertainty: None,
            },
        }
    }

    /// Create a FlowBudgetCharged fact with millisecond timestamp (backward compatibility)
    pub fn flow_budget_charged_ms(
        context_id: ContextId,
        authority_id: AuthorityId,
        amount: u32,
        spent_after: u64,
        charged_at_ms: u64,
    ) -> Self {
        Self::FlowBudgetCharged {
            context_id,
            authority_id,
            amount,
            spent_after,
            charged_at: PhysicalTime {
                ts_ms: charged_at_ms,
                uncertainty: None,
            },
        }
    }

    /// Create a HolePunchCompleted fact with millisecond timestamp (backward compatibility)
    pub fn hole_punch_completed_ms(
        session_id: String,
        context_id: ContextId,
        local_authority: AuthorityId,
        remote_authority: AuthorityId,
        success: bool,
        completed_at_ms: u64,
    ) -> Self {
        Self::HolePunchCompleted {
            session_id,
            context_id,
            local_authority,
            remote_authority,
            success,
            completed_at: PhysicalTime {
                ts_ms: completed_at_ms,
                uncertainty: None,
            },
        }
    }

    /// Get the fact type name for journal keying
    pub fn fact_type(&self) -> &'static str {
        match self {
            TransportFact::SessionEstablished { .. } => "session_established",
            TransportFact::SessionClosed { .. } => "session_closed",
            TransportFact::MessageSent { .. } => "message_sent",
            TransportFact::MessageReceived { .. } => "message_received",
            TransportFact::PeerDiscovered { .. } => "peer_discovered",
            TransportFact::ConnectionFailed { .. } => "connection_failed",
            TransportFact::FlowBudgetCharged { .. } => "flow_budget_charged",
            TransportFact::HolePunchCompleted { .. } => "hole_punch_completed",
        }
    }

    /// Encode this fact with a canonical envelope.
    pub fn to_bytes(&self) -> Vec<u8> {
        let message = VersionedMessage::new(self.clone(), Self::version())
            .with_metadata("type".to_string(), TRANSPORT_FACT_TYPE_ID.to_string());
        to_vec(&message).unwrap_or_default()
    }

    /// Decode a fact from a canonical envelope.
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        let message: VersionedMessage<Self> = from_slice(bytes).ok()?;
        if !message.version.is_compatible(&Self::version()) {
            return None;
        }
        Some(message.payload)
    }
}

/// Delta type for transport fact application
#[derive(Debug, Clone, Default)]
pub struct TransportFactDelta {
    /// Sessions established in this delta
    pub sessions_established: Vec<String>,
    /// Sessions closed in this delta
    pub sessions_closed: Vec<String>,
    /// Messages sent in this delta
    pub messages_sent: u64,
    /// Messages received in this delta
    pub messages_received: u64,
    /// Peers discovered in this delta
    pub peers_discovered: Vec<AuthorityId>,
    /// Total flow budget charged in this delta
    pub flow_budget_charged: u64,
}

impl FactDelta for TransportFactDelta {
    fn merge(&mut self, other: &Self) {
        self.sessions_established
            .extend(other.sessions_established.iter().cloned());
        self.sessions_closed
            .extend(other.sessions_closed.iter().cloned());
        self.messages_sent += other.messages_sent;
        self.messages_received += other.messages_received;
        self.peers_discovered
            .extend(other.peers_discovered.iter().cloned());
        self.flow_budget_charged += other.flow_budget_charged;
    }
}

/// Reducer for transport facts
#[derive(Debug, Clone, Default)]
pub struct TransportFactReducer;

impl TransportFactReducer {
    /// Create a new transport fact reducer
    pub fn new() -> Self {
        Self
    }
}

impl FactDeltaReducer<TransportFact, TransportFactDelta> for TransportFactReducer {
    fn apply(&self, fact: &TransportFact) -> TransportFactDelta {
        let mut delta = TransportFactDelta::default();

        match fact {
            TransportFact::SessionEstablished { session_id, .. } => {
                delta.sessions_established.push(session_id.clone());
            }
            TransportFact::SessionClosed { session_id, .. } => {
                delta.sessions_closed.push(session_id.clone());
            }
            TransportFact::MessageSent { .. } => {
                delta.messages_sent += 1;
            }
            TransportFact::MessageReceived { .. } => {
                delta.messages_received += 1;
            }
            TransportFact::PeerDiscovered { authority_id, .. } => {
                delta.peers_discovered.push(*authority_id);
            }
            TransportFact::FlowBudgetCharged { amount, .. } => {
                delta.flow_budget_charged += *amount as u64;
            }
            TransportFact::ConnectionFailed { .. } | TransportFact::HolePunchCompleted { .. } => {
                // These don't produce cumulative deltas
            }
        }

        delta
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::endpoint::EndpointAddress;
    use aura_core::types::facts::FactDeltaReducer;

    #[test]
    fn test_transport_fact_context_id() {
        let context_id = ContextId::new_from_entropy([1u8; 32]);
        let local = AuthorityId::new_from_entropy([2u8; 32]);
        let remote = AuthorityId::new_from_entropy([3u8; 32]);

        let fact = TransportFact::session_established_ms(
            "test-session".to_string(),
            context_id,
            local,
            remote,
            TransportProtocol::Tcp {
                endpoint: EndpointAddress::new("127.0.0.1:8080"),
            },
            1000,
        );

        assert_eq!(fact.context_id(), context_id);
        assert_eq!(fact.timestamp_ms(), 1000);
        assert_eq!(fact.fact_type(), "session_established");
    }

    #[test]
    fn test_transport_fact_reducer() {
        let reducer = TransportFactReducer::new();
        let context_id = ContextId::new_from_entropy([1u8; 32]);
        let authority = AuthorityId::new_from_entropy([2u8; 32]);

        let fact = TransportFact::flow_budget_charged_ms(context_id, authority, 100, 500, 1000);

        let delta = reducer.apply(&fact);
        assert_eq!(delta.flow_budget_charged, 100);
    }

    #[test]
    fn test_timestamp_ms_backward_compat() {
        let context_id = ContextId::new_from_entropy([1u8; 32]);
        let authority = AuthorityId::new_from_entropy([2u8; 32]);

        let fact = TransportFact::peer_discovered_ms(context_id, authority, vec![], 1234567890);
        assert_eq!(fact.timestamp_ms(), 1234567890);
    }
}
