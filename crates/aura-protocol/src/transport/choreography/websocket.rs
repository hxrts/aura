//! WebSocket Choreographic Protocols
//!
//! Layer 4: Multi-party WebSocket coordination using choreographic protocols.
//! YES choreography - complex handshake and session management with multiple phases.
//! Target: <250 lines, focused on choreographic coordination.

use super::{ChoreographicConfig, ChoreographicError, ChoreographicResult};
use aura_core::{ContextId, DeviceId};
use aura_macros::choreography;
use crate::handlers::{AuraHandlerError, EffectType, ExecutionMode};
use crate::handlers::core::AuraHandler;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::SystemTime;

/// WebSocket handshake coordinator using choreographic protocols
#[derive(Debug, Clone)]
pub struct WebSocketHandshakeCoordinator {
    device_id: DeviceId,
    config: ChoreographicConfig,
    active_handshakes: HashMap<String, HandshakeState>,
}

/// WebSocket session coordinator for active connections
#[derive(Debug, Clone)]
pub struct WebSocketSessionCoordinator {
    device_id: DeviceId,
    config: ChoreographicConfig,
    active_sessions: HashMap<String, SessionState>,
}

/// Handshake state tracking
#[derive(Debug, Clone)]
struct HandshakeState {
    session_id: String,
    peer_id: DeviceId,
    phase: HandshakePhase,
    started_at: SystemTime,
    capabilities: Vec<String>,
}

/// Session state tracking
#[derive(Debug, Clone)]
struct SessionState {
    session_id: String,
    peer_id: DeviceId,
    established_at: SystemTime,
    last_activity: SystemTime,
    message_count: u64,
}

/// Handshake phase enumeration
#[derive(Debug, Clone, PartialEq, Eq)]
enum HandshakePhase {
    Initiated,
    CapabilityNegotiation,
    SecuritySetup,
    Confirmation,
    Completed,
    Failed(String),
}

/// WebSocket handshake initiation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebSocketHandshakeInit {
    pub session_id: String,
    pub initiator_id: DeviceId,
    pub websocket_url: String,
    pub supported_protocols: Vec<String>,
    pub capabilities: Vec<String>,
    pub context_id: ContextId,
}

/// WebSocket handshake response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebSocketHandshakeResponse {
    pub session_id: String,
    pub responder_id: DeviceId,
    pub accepted_protocols: Vec<String>,
    pub granted_capabilities: Vec<String>,
    pub handshake_result: WebSocketHandshakeResult,
}

/// WebSocket session data message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebSocketSessionData {
    pub session_id: String,
    pub sender_id: DeviceId,
    pub message_type: MessageType,
    pub payload: Vec<u8>,
    pub sequence_number: u64,
    pub timestamp: SystemTime,
}

/// WebSocket session teardown request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebSocketTeardown {
    pub session_id: String,
    pub initiator_id: DeviceId,
    pub reason: String,
    pub graceful: bool,
}

/// Message type enumeration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MessageType {
    Data,
    Control,
    Keepalive,
    Error,
}

/// Handshake result enumeration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WebSocketHandshakeResult {
    Success,
    ProtocolMismatch { supported: Vec<String> },
    CapabilityDenied { missing: Vec<String> },
    SecurityError { reason: String },
    Rejected { reason: String },
}

impl WebSocketHandshakeCoordinator {
    /// Create new WebSocket handshake coordinator
    pub fn new(device_id: DeviceId, config: ChoreographicConfig) -> Self {
        Self {
            device_id,
            config,
            active_handshakes: HashMap::new(),
        }
    }

    /// Initiate WebSocket handshake
    pub fn initiate_handshake(
        &mut self,
        peer_id: DeviceId,
        websocket_url: String,
        context_id: ContextId,
    ) -> ChoreographicResult<String> {
        if self.active_handshakes.len() >= self.config.max_concurrent_protocols {
            return Err(ChoreographicError::ExecutionFailed(
                "Maximum concurrent handshakes exceeded".to_string(),
            ));
        }

        let session_id = format!(
            "ws-session-{}-{}",
            format!("{:?}", self.device_id)[..8].to_string(),
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis()
        );

        let handshake_state = HandshakeState {
            session_id: session_id.clone(),
            peer_id,
            phase: HandshakePhase::Initiated,
            started_at: SystemTime::now(),
            capabilities: self.config.required_capabilities.clone(),
        };

        self.active_handshakes
            .insert(session_id.clone(), handshake_state);
        Ok(session_id)
    }

    /// Process handshake response
    pub fn process_handshake_response(
        &mut self,
        response: &WebSocketHandshakeResponse,
    ) -> ChoreographicResult<bool> {
        let handshake = self
            .active_handshakes
            .get_mut(&response.session_id)
            .ok_or_else(|| {
                ChoreographicError::ExecutionFailed(format!(
                    "Handshake not found: {}",
                    response.session_id
                ))
            })?;

        match &response.handshake_result {
            WebSocketHandshakeResult::Success => {
                handshake.phase = HandshakePhase::Completed;
                Ok(true)
            }
            WebSocketHandshakeResult::ProtocolMismatch { .. } => {
                handshake.phase = HandshakePhase::Failed("Protocol mismatch".to_string());
                Ok(false)
            }
            WebSocketHandshakeResult::CapabilityDenied { missing } => {
                handshake.phase =
                    HandshakePhase::Failed(format!("Missing capabilities: {:?}", missing));
                Ok(false)
            }
            WebSocketHandshakeResult::SecurityError { reason } => {
                handshake.phase = HandshakePhase::Failed(format!("Security error: {}", reason));
                Ok(false)
            }
            WebSocketHandshakeResult::Rejected { reason } => {
                handshake.phase = HandshakePhase::Failed(format!("Handshake rejected: {}", reason));
                Ok(false)
            }
        }
    }

    /// Get handshake state
    pub fn get_handshake_state(&self, session_id: &str) -> Option<&HandshakePhase> {
        self.active_handshakes.get(session_id).map(|h| &h.phase)
    }

    /// Clean up completed handshakes
    pub fn cleanup_completed(&mut self) -> usize {
        let initial_count = self.active_handshakes.len();

        self.active_handshakes.retain(|_, handshake| {
            !matches!(
                handshake.phase,
                HandshakePhase::Completed | HandshakePhase::Failed(_)
            )
        });

        initial_count - self.active_handshakes.len()
    }
}

impl WebSocketSessionCoordinator {
    /// Create new WebSocket session coordinator
    pub fn new(device_id: DeviceId, config: ChoreographicConfig) -> Self {
        Self {
            device_id,
            config,
            active_sessions: HashMap::new(),
        }
    }

    /// Establish session from completed handshake
    pub fn establish_session(
        &mut self,
        session_id: String,
        peer_id: DeviceId,
    ) -> ChoreographicResult<()> {
        let session_state = SessionState {
            session_id: session_id.clone(),
            peer_id,
            established_at: SystemTime::now(),
            last_activity: SystemTime::now(),
            message_count: 0,
        };

        self.active_sessions.insert(session_id, session_state);
        Ok(())
    }

    /// Record session activity
    pub fn record_activity(&mut self, session_id: &str) -> ChoreographicResult<()> {
        let session = self.active_sessions.get_mut(session_id).ok_or_else(|| {
            ChoreographicError::ExecutionFailed(format!("Session not found: {}", session_id))
        })?;

        session.last_activity = SystemTime::now();
        session.message_count += 1;
        Ok(())
    }

    /// List active sessions
    pub fn list_sessions(&self) -> Vec<&SessionState> {
        self.active_sessions.values().collect()
    }

    /// Terminate session
    pub fn terminate_session(&mut self, session_id: &str) -> ChoreographicResult<()> {
        self.active_sessions.remove(session_id);
        Ok(())
    }
}

// Choreographic Protocol Definitions
mod websocket_handshake {
    use super::*;
    
    // Multi-party WebSocket handshake with capability negotiation
    choreography! {
        #[namespace = "websocket_handshake"]
        protocol WebSocketHandshakeProtocol {
            roles: Initiator, Responder;

            // Phase 1: Handshake initiation with capability advertisement
            Initiator[guard_capability = "initiate_websocket_handshake",
                      flow_cost = 150,
                      journal_facts = "websocket_handshake_initiated"]
            -> Responder: WebSocketHandshakeInit(WebSocketHandshakeInit);

            // Phase 2: Responder processes and responds with capability grant/deny
            Responder[guard_capability = "respond_websocket_handshake",
                      flow_cost = 120,
                      journal_facts = "websocket_handshake_processed"]
            -> Initiator: WebSocketHandshakeResponse(WebSocketHandshakeResponse);
        }
    }
}

mod websocket_session {
    use super::*;
    
    // Active WebSocket session coordination
    choreography! {
        #[namespace = "websocket_session"]
        protocol WebSocketActiveSession {
            roles: Peer1, Peer2;

            // Data exchange with flow control
            Peer1[guard_capability = "send_websocket_data",
                  flow_cost = 50,
                  journal_facts = "websocket_data_sent"]
            -> Peer2: WebSocketSessionData(WebSocketSessionData);

            Peer2[guard_capability = "send_websocket_data",
                  flow_cost = 50,
                  journal_facts = "websocket_data_sent"]
            -> Peer1: WebSocketSessionData(WebSocketSessionData);
        }
    }
}

mod websocket_teardown {
    use super::*;
    
    // Graceful WebSocket teardown coordination
    choreography! {
    #[namespace = "websocket_teardown"]
    protocol WebSocketTeardownProtocol {
        roles: Initiator, Responder;

        // Graceful teardown initiation
        Initiator[guard_capability = "initiate_websocket_teardown",
                  flow_cost = 80,
                  journal_facts = "websocket_teardown_initiated"]
        -> Responder: WebSocketTeardown(WebSocketTeardown);

        // Teardown acknowledgment
        Responder[guard_capability = "acknowledge_websocket_teardown",
                  flow_cost = 60,
                  journal_facts = "websocket_teardown_acknowledged"]
        -> Initiator: WebSocketTeardown(WebSocketTeardown);
    }
}
}
