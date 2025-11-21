//! Rendezvous Protocol Messages
//!
//! Implements the Offer/Answer exchange for establishing direct connections
//! between devices from different accounts. The protocol flow is:
//!
//! 1. Device A publishes Offer envelope with available transports
//! 2. Device B recognizes Offer, selects transport, publishes Answer
//! 3. Both devices perform PSK-bound handshake on selected transport
//!
//! Reference: docs/051_rendezvous.md Section 4.3

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Transport type enumeration for rendezvous
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum TransportKind {
    Quic,
    WebSocket,
    WebRtc,
    Tor,
    Ble,
}

/// Transport configuration and metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransportDescriptor {
    pub kind: TransportKind,
    pub metadata: BTreeMap<String, String>,
    /// Local addresses (direct connectivity)
    pub local_addresses: Vec<String>,
    /// Reflexive addresses discovered via STUN (NAT traversal)
    pub reflexive_addresses: Vec<String>,
}

impl TransportDescriptor {
    pub fn quic(local_addr: String, alpn: String) -> Self {
        let mut metadata = BTreeMap::new();
        metadata.insert("alpn".to_string(), alpn);
        Self {
            kind: TransportKind::Quic,
            metadata,
            local_addresses: vec![local_addr],
            reflexive_addresses: vec![],
        }
    }

    pub fn quic_with_stun(local_addr: String, reflexive_addr: String, alpn: String) -> Self {
        let mut metadata = BTreeMap::new();
        metadata.insert("alpn".to_string(), alpn);
        Self {
            kind: TransportKind::Quic,
            metadata,
            local_addresses: vec![local_addr],
            reflexive_addresses: vec![reflexive_addr],
        }
    }

    pub fn websocket(local_addr: String) -> Self {
        let metadata = BTreeMap::new();
        Self {
            kind: TransportKind::WebSocket,
            metadata,
            local_addresses: vec![local_addr],
            reflexive_addresses: vec![],
        }
    }

    pub fn webrtc(ufrag: String, pwd: String, candidates: Vec<String>) -> Self {
        let mut metadata = BTreeMap::new();
        metadata.insert("ufrag".to_string(), ufrag);
        metadata.insert("pwd".to_string(), pwd);
        metadata.insert("candidates".to_string(), candidates.join(","));
        Self {
            kind: TransportKind::WebRtc,
            metadata,
            local_addresses: vec![],
            reflexive_addresses: vec![],
        }
    }

    pub fn tor(onion: String) -> Self {
        let mut metadata = BTreeMap::new();
        metadata.insert("onion".to_string(), onion.clone());
        Self {
            kind: TransportKind::Tor,
            metadata,
            local_addresses: vec![onion],
            reflexive_addresses: vec![],
        }
    }

    pub fn ble(service_uuid: String) -> Self {
        let mut metadata = BTreeMap::new();
        metadata.insert("service_uuid".to_string(), service_uuid);
        Self {
            kind: TransportKind::Ble,
            metadata,
            local_addresses: vec![],
            reflexive_addresses: vec![],
        }
    }

    /// Add a reflexive address discovered via STUN
    pub fn add_reflexive_address(&mut self, reflexive_addr: String) {
        if !self.reflexive_addresses.contains(&reflexive_addr) {
            self.reflexive_addresses.push(reflexive_addr);
        }
    }

    /// Get all available addresses (local + reflexive) for connection attempts
    pub fn get_all_addresses(&self) -> Vec<String> {
        let mut addresses = self.local_addresses.clone();
        addresses.extend(self.reflexive_addresses.clone());
        addresses
    }

    /// Get priority-ordered addresses (reflexive first, then local)
    pub fn get_priority_addresses(&self) -> Vec<String> {
        let mut addresses = self.reflexive_addresses.clone();
        addresses.extend(self.local_addresses.clone());
        addresses
    }
}

/// Message type enumeration for rendezvous payloads
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum PayloadKind {
    Offer,
    Answer,
    Ack,
    Rekey,
    RevokeDevice,
}

/// Authentication payload for rendezvous messages
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthenticationPayload {
    pub kind: PayloadKind,
    pub ver: u8,
    pub device_cert: Vec<u8>,
    pub channel_binding: [u8; 32],
    pub expires: u64,
    pub counter: u32,
    pub inner_sig: Vec<u8>,
}

impl AuthenticationPayload {
    pub fn new(
        kind: PayloadKind,
        device_cert: Vec<u8>,
        channel_binding: [u8; 32],
        expires: u64,
        counter: u32,
        inner_sig: Vec<u8>,
    ) -> Self {
        Self {
            kind,
            ver: 1,
            device_cert,
            channel_binding,
            expires,
            counter,
            inner_sig,
        }
    }

    pub fn compute_channel_binding(k_psk: &[u8; 32], device_static_pub: &[u8]) -> [u8; 32] {
        let mut data = Vec::new();
        data.extend_from_slice(k_psk);
        data.extend_from_slice(device_static_pub);
        aura_core::hash::hash(&data)
    }
}

/// Storage capability announcement for peer discovery
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StorageCapabilityAnnouncement {
    pub available_capacity_bytes: u64,
    pub max_chunk_size: u32,
    pub accepting_new_relationships: bool,
}

/// Transport offer payload with capability announcements
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransportOfferPayload {
    pub transports: Vec<TransportDescriptor>,
    pub selected_transport: Option<u8>,
    pub required_permissions: Vec<String>,
    pub capability_proof: Option<Vec<u8>>,
    pub storage_announcement: Option<StorageCapabilityAnnouncement>,
    /// Punch nonce for simultaneous open coordination (hole-punching)
    pub punch_nonce: Option<[u8; 32]>,
}

impl TransportOfferPayload {
    pub fn new_offer(
        transports: Vec<TransportDescriptor>,
        required_permissions: Vec<String>,
    ) -> Self {
        Self {
            transports,
            selected_transport: None,
            required_permissions,
            capability_proof: None,
            storage_announcement: None,
            punch_nonce: None,
        }
    }

    pub fn new_offer_with_storage(
        transports: Vec<TransportDescriptor>,
        required_permissions: Vec<String>,
        storage_announcement: StorageCapabilityAnnouncement,
    ) -> Self {
        Self {
            transports,
            selected_transport: None,
            required_permissions,
            capability_proof: None,
            storage_announcement: Some(storage_announcement),
            punch_nonce: None,
        }
    }

    pub fn new_answer(original_transports: Vec<TransportDescriptor>, selected_index: u8) -> Self {
        Self {
            transports: original_transports,
            selected_transport: Some(selected_index),
            required_permissions: vec![],
            capability_proof: None,
            storage_announcement: None,
            punch_nonce: None,
        }
    }

    pub fn new_answer_with_storage(
        original_transports: Vec<TransportDescriptor>,
        selected_index: u8,
        storage_announcement: StorageCapabilityAnnouncement,
    ) -> Self {
        Self {
            transports: original_transports,
            selected_transport: Some(selected_index),
            required_permissions: vec![],
            capability_proof: None,
            storage_announcement: Some(storage_announcement),
            punch_nonce: None,
        }
    }

    /// Create offer with punch nonce for hole-punching coordination
    pub fn new_offer_with_punch(
        transports: Vec<TransportDescriptor>,
        required_permissions: Vec<String>,
        punch_nonce: [u8; 32],
    ) -> Self {
        Self {
            transports,
            selected_transport: None,
            required_permissions,
            capability_proof: None,
            storage_announcement: None,
            punch_nonce: Some(punch_nonce),
        }
    }

    /// Create answer with punch nonce for coordinated hole-punching
    pub fn new_answer_with_punch(
        original_transports: Vec<TransportDescriptor>,
        selected_index: u8,
        punch_nonce: [u8; 32],
    ) -> Self {
        Self {
            transports: original_transports,
            selected_transport: Some(selected_index),
            required_permissions: vec![],
            capability_proof: None,
            storage_announcement: None,
            punch_nonce: Some(punch_nonce),
        }
    }

    /// Set punch nonce for existing payload
    pub fn with_punch_nonce(mut self, punch_nonce: [u8; 32]) -> Self {
        self.punch_nonce = Some(punch_nonce);
        self
    }

    /// Get punch nonce if available
    pub fn get_punch_nonce(&self) -> Option<[u8; 32]> {
        self.punch_nonce
    }
}

/// Complete rendezvous message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RendezvousMessage {
    pub auth: AuthenticationPayload,
    pub transport: TransportOfferPayload,
}

/// PSK handshake transcript for verification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HandshakeTranscript {
    pub device_cert_a: Vec<u8>,
    pub device_cert_b: Vec<u8>,
    pub channel_binding: [u8; 32],
    pub transport_descriptor: Vec<u8>,
    pub offer_counter: u32,
    pub answer_counter: u32,
}

impl HandshakeTranscript {
    pub fn new(
        device_cert_a: Vec<u8>,
        device_cert_b: Vec<u8>,
        channel_binding: [u8; 32],
        transport_descriptor: Vec<u8>,
        offer_counter: u32,
        answer_counter: u32,
    ) -> Self {
        Self {
            device_cert_a,
            device_cert_b,
            channel_binding,
            transport_descriptor,
            offer_counter,
            answer_counter,
        }
    }

    /// Compute transcript binding for verification
    pub fn compute_binding(&self) -> [u8; 32] {
        let mut data = Vec::new();
        data.extend_from_slice(&self.device_cert_a);
        data.extend_from_slice(&self.device_cert_b);
        data.extend_from_slice(&self.channel_binding);
        data.extend_from_slice(&self.transport_descriptor);
        data.extend_from_slice(&self.offer_counter.to_le_bytes());
        data.extend_from_slice(&self.answer_counter.to_le_bytes());
        aura_core::hash::hash(&data)
    }
}

/// PSK handshake configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PskHandshakeConfig {
    pub k_psk: [u8; 32],
    pub expected_peer_device_id: aura_core::identifiers::DeviceId,
    pub local_device_cert: Vec<u8>,
    pub transport_descriptor: TransportDescriptor,
}

/// Handshake completion result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HandshakeResult {
    pub success: bool,
    pub transcript: Option<HandshakeTranscript>,
    pub session_key: Option<[u8; 32]>,
    pub error_message: Option<String>,
}

// All message types use standard serde traits for serialization
