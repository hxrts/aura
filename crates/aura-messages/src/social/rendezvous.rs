//! Rendezvous Protocol Messages
//!
//! Implements the SSB Offer/Answer exchange for establishing direct connections
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
    WebRtc,
    Tor,
    Ble,
}

/// Transport configuration and metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransportDescriptor {
    pub kind: TransportKind,
    pub metadata: BTreeMap<String, String>,
}

impl TransportDescriptor {
    pub fn quic(addr: String, alpn: String) -> Self {
        let mut metadata = BTreeMap::new();
        metadata.insert("addr".to_string(), addr);
        metadata.insert("alpn".to_string(), alpn);
        Self {
            kind: TransportKind::Quic,
            metadata,
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
        }
    }

    pub fn tor(onion: String) -> Self {
        let mut metadata = BTreeMap::new();
        metadata.insert("onion".to_string(), onion);
        Self {
            kind: TransportKind::Tor,
            metadata,
        }
    }

    pub fn ble(service_uuid: String) -> Self {
        let mut metadata = BTreeMap::new();
        metadata.insert("service_uuid".to_string(), service_uuid);
        Self {
            kind: TransportKind::Ble,
            metadata,
        }
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
        *blake3::hash(&data).as_bytes()
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
        }
    }

    pub fn new_answer(original_transports: Vec<TransportDescriptor>, selected_index: u8) -> Self {
        Self {
            transports: original_transports,
            selected_transport: Some(selected_index),
            required_permissions: vec![],
            capability_proof: None,
            storage_announcement: None,
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
        }
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
        *blake3::hash(&data).as_bytes()
    }
}

/// PSK handshake configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PskHandshakeConfig {
    pub k_psk: [u8; 32],
    pub expected_peer_device_id: aura_types::DeviceId,
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
