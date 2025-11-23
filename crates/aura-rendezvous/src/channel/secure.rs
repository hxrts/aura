//! Secure Channel Establishment Protocol
//!
//! Layer 5: Complete secure channel protocol implementation.
//! Feature crate: Complete end-to-end secure channel establishment using choreography.
//! Target: <300 lines, focused on domain-specific transport security.

use aura_core::{AuraError, ContextId, DeviceId};
use aura_macros::choreography;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::SystemTime;

/// Secure channel coordinator using choreographic protocols
#[derive(Debug, Clone)]
pub struct SecureChannelCoordinator {
    device_id: DeviceId,
    channel_config: ChannelConfig,
    active_channels: HashMap<String, ChannelState>,
}

/// Configuration for secure channels
#[derive(Debug, Clone)]
pub struct ChannelConfig {
    pub handshake_timeout: std::time::Duration,
    pub key_rotation_interval: std::time::Duration,
    pub max_concurrent_channels: usize,
}

/// Secure channel state tracking
#[derive(Debug, Clone)]
pub struct ChannelState {
    pub channel_id: String,
    pub peer_id: DeviceId,
    pub state: ChannelLifecycleState,
    pub established_at: Option<SystemTime>,
    pub last_rotation: Option<SystemTime>,
}

/// Channel lifecycle states
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChannelLifecycleState {
    Initiating,
    HandshakeInProgress,
    Established,
    Rotating,
    Closing,
    Closed,
    Error(String),
}

/// Handshake initiation request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HandshakeInit {
    pub channel_id: String,
    pub initiator_id: DeviceId,
    pub proposed_algorithms: Vec<String>,
    pub initial_public_key: Vec<u8>,
    pub capabilities: Vec<String>,
    pub context_id: ContextId,
}

/// Handshake response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HandshakeResponse {
    pub channel_id: String,
    pub responder_id: DeviceId,
    pub selected_algorithms: Vec<String>,
    pub response_public_key: Vec<u8>,
    pub accepted_capabilities: Vec<String>,
    pub handshake_result: HandshakeResult,
}

/// Handshake completion confirmation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HandshakeComplete {
    pub channel_id: String,
    pub shared_secret_hash: Vec<u8>,
    pub established_at: SystemTime,
    pub rotation_schedule: Option<SystemTime>,
}

/// Handshake result enumeration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum HandshakeResult {
    Accept,
    Reject { reason: String },
    RequestRenegotiation { preferred_algorithms: Vec<String> },
}

/// Key rotation request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyRotationRequest {
    pub channel_id: String,
    pub new_public_key: Vec<u8>,
    pub rotation_nonce: Vec<u8>,
    pub timestamp: SystemTime,
}

impl Default for ChannelConfig {
    fn default() -> Self {
        Self {
            handshake_timeout: std::time::Duration::from_secs(30),
            key_rotation_interval: std::time::Duration::from_secs(3600), // 1 hour
            max_concurrent_channels: 50,
        }
    }
}

impl SecureChannelCoordinator {
    /// Create new secure channel coordinator
    pub fn new(device_id: DeviceId, config: ChannelConfig) -> Self {
        Self {
            device_id,
            channel_config: config,
            active_channels: HashMap::new(),
        }
    }

    /// Get device ID
    pub fn device_id(&self) -> DeviceId {
        self.device_id
    }

    /// Initialize new channel
    pub fn init_channel(
        &mut self,
        peer_id: DeviceId,
        _context_id: ContextId,
    ) -> Result<String, AuraError> {
        if self.active_channels.len() >= self.channel_config.max_concurrent_channels {
            return Err(AuraError::invalid("Maximum concurrent channels exceeded"));
        }

        let channel_id = format!(
            "channel-{}-{}",
            &format!("{:?}", self.device_id)[..8],
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis()
        );

        let channel_state = ChannelState {
            channel_id: channel_id.clone(),
            peer_id,
            state: ChannelLifecycleState::Initiating,
            established_at: None,
            last_rotation: None,
        };

        self.active_channels
            .insert(channel_id.clone(), channel_state);
        Ok(channel_id)
    }

    /// Process handshake response
    pub fn process_handshake_response(
        &mut self,
        response: &HandshakeResponse,
    ) -> Result<bool, AuraError> {
        let channel = self
            .active_channels
            .get_mut(&response.channel_id)
            .ok_or_else(|| {
                AuraError::not_found(format!("Channel not found: {}", response.channel_id))
            })?;

        match &response.handshake_result {
            HandshakeResult::Accept => {
                channel.state = ChannelLifecycleState::Established;
                channel.established_at = Some(SystemTime::now());
                Ok(true)
            }
            HandshakeResult::Reject { reason } => {
                channel.state = ChannelLifecycleState::Error(reason.clone());
                Ok(false)
            }
            HandshakeResult::RequestRenegotiation { .. } => {
                channel.state = ChannelLifecycleState::HandshakeInProgress;
                Ok(false) // Need to renegotiate
            }
        }
    }

    /// Check if key rotation is needed
    pub fn needs_key_rotation(&self, channel_id: &str) -> bool {
        if let Some(channel) = self.active_channels.get(channel_id) {
            if let Some(last_rotation) = channel.last_rotation {
                return last_rotation.elapsed().unwrap_or_default()
                    >= self.channel_config.key_rotation_interval;
            }
            if let Some(established_at) = channel.established_at {
                return established_at.elapsed().unwrap_or_default()
                    >= self.channel_config.key_rotation_interval;
            }
        }
        false
    }

    /// Get channel state
    pub fn get_channel_state(&self, channel_id: &str) -> Option<&ChannelState> {
        self.active_channels.get(channel_id)
    }

    /// List active channels
    pub fn list_channels(&self) -> Vec<&ChannelState> {
        self.active_channels.values().collect()
    }

    /// Close channel
    pub fn close_channel(&mut self, channel_id: &str) -> Result<(), AuraError> {
        if let Some(mut channel) = self.active_channels.remove(channel_id) {
            channel.state = ChannelLifecycleState::Closed;
        }
        Ok(())
    }
}

// Choreographic Protocol Definitions
mod secure_channel_establishment {
    use super::*;

    // Multi-phase secure channel establishment with choices
    choreography! {
        #[namespace = "secure_channel"]
        protocol SecureChannelEstablishment {
            roles: Initiator, Responder;

            // Phase 1: Handshake initiation
            Initiator[guard_capability = "initiate_handshake",
                      flow_cost = 150,
                      journal_facts = "handshake_initiated"]
            -> Responder: HandshakeInit(HandshakeInit);

            // Phase 2: Responder choice - accept, reject, or renegotiate
            choice Responder {
                accept: {
                    Responder[guard_capability = "accept_handshake",
                              flow_cost = 100,
                              journal_facts = "handshake_accepted"]
                    -> Initiator: HandshakeResponse(HandshakeResponse);

                    // Phase 3a: Handshake completion
                    Initiator[guard_capability = "complete_handshake",
                              flow_cost = 75,
                              journal_facts = "channel_established"]
                    -> Responder: HandshakeComplete(HandshakeComplete);
                }
                reject: {
                    Responder[guard_capability = "reject_handshake",
                              flow_cost = 50,
                              journal_facts = "handshake_rejected"]
                    -> Initiator: HandshakeResponse(HandshakeResponse);
                }
                renegotiate: {
                    Responder[guard_capability = "request_renegotiation",
                              flow_cost = 80,
                              journal_facts = "renegotiation_requested"]
                    -> Initiator: HandshakeResponse(HandshakeResponse);

                    // Phase 3b: Renegotiation loop (simplified)
                    Initiator[guard_capability = "initiate_handshake",
                              flow_cost = 120]
                    -> Responder: HandshakeInit(HandshakeInit);
                }
            }
        }
    }
}

mod key_rotation {
    use super::*;

    // Separate choreography for key rotation
    choreography! {
        #[namespace = "key_rotation"]
        protocol KeyRotationProtocol {
            roles: ChannelPeer1, ChannelPeer2;

            // Coordinated key rotation
            ChannelPeer1[guard_capability = "rotate_keys",
                         flow_cost = 100,
                         journal_facts = "key_rotation_initiated"]
            -> ChannelPeer2: KeyRotationRequest(KeyRotationRequest);

            ChannelPeer2[guard_capability = "confirm_rotation",
                         flow_cost = 80,
                         journal_facts = "key_rotation_confirmed"]
            -> ChannelPeer1: KeyRotationRequest(KeyRotationRequest);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_channel_initialization() {
        let mut coordinator =
            SecureChannelCoordinator::new(DeviceId::from([1u8; 32]), ChannelConfig::default());

        let peer_id = DeviceId::from([2u8; 32]);
        let context_id = ContextId::new();

        let result = coordinator.init_channel(peer_id, context_id);
        assert!(result.is_ok());

        let channel_id = result.unwrap();
        let channel_state = coordinator.get_channel_state(&channel_id);
        assert!(channel_state.is_some());
        assert_eq!(channel_state.unwrap().peer_id, peer_id);
    }

    #[test]
    fn test_handshake_acceptance() {
        let mut coordinator =
            SecureChannelCoordinator::new(DeviceId::from([1u8; 32]), ChannelConfig::default());

        let peer_id = DeviceId::from([2u8; 32]);
        let context_id = ContextId::new();
        let channel_id = coordinator.init_channel(peer_id, context_id).unwrap();

        let response = HandshakeResponse {
            channel_id: channel_id.clone(),
            responder_id: peer_id,
            selected_algorithms: vec!["aes256".to_string()],
            response_public_key: vec![1, 2, 3, 4],
            accepted_capabilities: vec!["secure_transport".to_string()],
            handshake_result: HandshakeResult::Accept,
        };

        let result = coordinator.process_handshake_response(&response);
        assert!(result.is_ok());
        assert!(result.unwrap()); // Should be true for accept

        let channel_state = coordinator.get_channel_state(&channel_id).unwrap();
        assert_eq!(channel_state.state, ChannelLifecycleState::Established);
        assert!(channel_state.established_at.is_some());
    }
}
