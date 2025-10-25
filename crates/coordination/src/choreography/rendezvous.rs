//! Rendezvous Protocol Choreography
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum TransportKind {
    Quic,
    WebRtc,
    Tor,
    Ble,
}

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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum PayloadKind {
    Offer,
    Answer,
    Ack,
    Rekey,
    RevokeDevice,
}

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
        use blake3::Hasher;
        let mut hasher = Hasher::new();
        hasher.update(k_psk);
        hasher.update(device_static_pub);
        let hash = hasher.finalize();
        let mut binding = [0u8; 32];
        binding.copy_from_slice(hash.as_bytes());
        binding
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransportOfferPayload {
    pub transports: Vec<TransportDescriptor>,
    pub selected_transport: Option<u8>,
    pub required_permissions: Vec<String>,
    pub capability_proof: Option<Vec<u8>>,
    pub storage_announcement: Option<StorageCapabilityAnnouncement>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StorageCapabilityAnnouncement {
    pub available_capacity_bytes: u64,
    pub max_chunk_size: u32,
    pub accepting_new_relationships: bool,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RendezvousEnvelope {
    pub auth: AuthenticationPayload,
    pub transport: TransportOfferPayload,
}

#[derive(Debug)]
pub enum RendezvousError {
    NoTransportsAvailable,
    NoCompatibleTransport,
    InvalidAuthentication,
    ExpiredOffer,
    HandshakeFailed(String),
    ChannelBindingMismatch,
    TranscriptBindingMismatch,
}

impl std::fmt::Display for RendezvousError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoTransportsAvailable => write!(f, "No transports available"),
            Self::NoCompatibleTransport => write!(f, "No compatible transport found"),
            Self::InvalidAuthentication => write!(f, "Invalid authentication"),
            Self::ExpiredOffer => write!(f, "Offer has expired"),
            Self::HandshakeFailed(msg) => write!(f, "Handshake failed: {}", msg),
            Self::ChannelBindingMismatch => write!(f, "Channel binding mismatch"),
            Self::TranscriptBindingMismatch => write!(f, "Transcript binding mismatch"),
        }
    }
}

impl std::error::Error for RendezvousError {}

pub type Result<T> = std::result::Result<T, RendezvousError>;

#[derive(Debug, Clone)]
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

    pub fn compute_binding(&self) -> [u8; 32] {
        use blake3::Hasher;
        let mut hasher = Hasher::new();
        hasher.update(&self.device_cert_a);
        hasher.update(&self.device_cert_b);
        hasher.update(&self.channel_binding);
        hasher.update(&self.transport_descriptor);
        hasher.update(&self.offer_counter.to_le_bytes());
        hasher.update(&self.answer_counter.to_le_bytes());
        hasher.update(b"rendezvous-transcript");

        let hash = hasher.finalize();
        let mut binding = [0u8; 32];
        binding.copy_from_slice(hash.as_bytes());
        binding
    }
}

#[derive(Debug, Clone)]
pub struct PskHandshakeConfig {
    pub k_psk: [u8; 32],
    pub device_static_priv: Vec<u8>,
    pub device_static_pub: Vec<u8>,
    pub peer_static_pub: Vec<u8>,
    pub is_initiator: bool,
}

#[derive(Debug, Clone)]
pub struct HandshakeResult {
    pub session_key: [u8; 32],
    pub transcript_binding: [u8; 32],
    pub peer_device_cert: Vec<u8>,
}

pub struct RendezvousProtocol;

impl RendezvousProtocol {
    pub fn create_offer_envelope(
        device_cert: Vec<u8>,
        k_psk: &[u8; 32],
        device_static_pub: &[u8],
        transports: Vec<TransportDescriptor>,
        counter: u32,
        expires: u64,
        device_priv_key: &[u8],
    ) -> Result<RendezvousEnvelope> {
        if transports.is_empty() {
            return Err(RendezvousError::NoTransportsAvailable);
        }

        let channel_binding =
            AuthenticationPayload::compute_channel_binding(k_psk, device_static_pub);

        let inner_sig = Self::sign_payload(
            &PayloadKind::Offer,
            &channel_binding,
            counter,
            expires,
            device_priv_key,
        );

        let auth = AuthenticationPayload::new(
            PayloadKind::Offer,
            device_cert,
            channel_binding,
            expires,
            counter,
            inner_sig,
        );

        let transport = TransportOfferPayload::new_offer(transports, vec![]);

        Ok(RendezvousEnvelope { auth, transport })
    }

    pub fn create_answer_envelope(
        device_cert: Vec<u8>,
        k_psk: &[u8; 32],
        device_static_pub: &[u8],
        offer_envelope: &RendezvousEnvelope,
        selected_transport_index: u8,
        counter: u32,
        expires: u64,
        device_priv_key: &[u8],
    ) -> Result<RendezvousEnvelope> {
        if (selected_transport_index as usize) >= offer_envelope.transport.transports.len() {
            return Err(RendezvousError::NoCompatibleTransport);
        }

        let channel_binding =
            AuthenticationPayload::compute_channel_binding(k_psk, device_static_pub);

        let inner_sig = Self::sign_payload(
            &PayloadKind::Answer,
            &channel_binding,
            counter,
            expires,
            device_priv_key,
        );

        let auth = AuthenticationPayload::new(
            PayloadKind::Answer,
            device_cert,
            channel_binding,
            expires,
            counter,
            inner_sig,
        );

        let transport = TransportOfferPayload::new_answer(
            offer_envelope.transport.transports.clone(),
            selected_transport_index,
        );

        Ok(RendezvousEnvelope { auth, transport })
    }

    pub fn verify_envelope(
        envelope: &RendezvousEnvelope,
        k_psk: &[u8; 32],
        peer_static_pub: &[u8],
        current_time: u64,
    ) -> Result<()> {
        if envelope.auth.expires < current_time {
            return Err(RendezvousError::ExpiredOffer);
        }

        let expected_binding =
            AuthenticationPayload::compute_channel_binding(k_psk, peer_static_pub);

        if envelope.auth.channel_binding != expected_binding {
            return Err(RendezvousError::ChannelBindingMismatch);
        }

        Ok(())
    }

    pub fn perform_psk_handshake(
        config: PskHandshakeConfig,
        transcript: HandshakeTranscript,
    ) -> Result<HandshakeResult> {
        let transcript_binding = transcript.compute_binding();

        let mut session_key_material = Vec::new();
        session_key_material.extend_from_slice(&config.k_psk);
        session_key_material.extend_from_slice(&config.device_static_pub);
        session_key_material.extend_from_slice(&config.peer_static_pub);
        session_key_material.extend_from_slice(&transcript_binding);

        let session_key = Self::derive_session_key(&session_key_material);

        let peer_device_cert = if config.is_initiator {
            transcript.device_cert_b.clone()
        } else {
            transcript.device_cert_a.clone()
        };

        Ok(HandshakeResult {
            session_key,
            transcript_binding,
            peer_device_cert,
        })
    }

    fn sign_payload(
        kind: &PayloadKind,
        channel_binding: &[u8; 32],
        counter: u32,
        expires: u64,
        device_priv_key: &[u8],
    ) -> Vec<u8> {
        use blake3::Hasher;
        let mut hasher = Hasher::new();

        let kind_byte = match kind {
            PayloadKind::Offer => 0u8,
            PayloadKind::Answer => 1u8,
            PayloadKind::Ack => 2u8,
            PayloadKind::Rekey => 3u8,
            PayloadKind::RevokeDevice => 4u8,
        };

        hasher.update(&[kind_byte]);
        hasher.update(channel_binding);
        hasher.update(&counter.to_le_bytes());
        hasher.update(&expires.to_le_bytes());

        let payload_hash = hasher.finalize();

        let mut sig_hasher =
            Hasher::new_keyed(&device_priv_key[..32].try_into().unwrap_or([0u8; 32]));
        sig_hasher.update(payload_hash.as_bytes());
        sig_hasher.update(b"device-signature");

        sig_hasher.finalize().as_bytes()[..32].to_vec()
    }

    fn derive_session_key(material: &[u8]) -> [u8; 32] {
        use blake3::Hasher;
        let mut hasher = Hasher::new();
        hasher.update(material);
        hasher.update(b"session-key");
        let hash = hasher.finalize();
        let mut key = [0u8; 32];
        key.copy_from_slice(hash.as_bytes());
        key
    }
}

#[cfg(test)]
#[allow(warnings, clippy::all)]
mod tests {
    use super::*;

    fn create_test_transports() -> Vec<TransportDescriptor> {
        vec![
            TransportDescriptor::quic("203.0.113.4:6121".to_string(), "hq".to_string()),
            TransportDescriptor::tor("abcd.onion:443".to_string()),
        ]
    }

    #[test]
    fn test_create_offer_envelope() {
        let device_cert = vec![1u8; 64];
        let k_psk = [2u8; 32];
        let device_static_pub = vec![3u8; 32];
        let transports = create_test_transports();
        let device_priv_key = vec![4u8; 32];

        let result = RendezvousProtocol::create_offer_envelope(
            device_cert,
            &k_psk,
            &device_static_pub,
            transports,
            42,
            1000000,
            &device_priv_key,
        );

        assert!(result.is_ok());
        let envelope = result.unwrap();
        assert_eq!(envelope.auth.kind, PayloadKind::Offer);
        assert_eq!(envelope.auth.counter, 42);
        assert_eq!(envelope.transport.transports.len(), 2);
        assert!(envelope.transport.selected_transport.is_none());
    }

    #[test]
    fn test_create_answer_envelope() {
        let device_cert = vec![1u8; 64];
        let k_psk = [2u8; 32];
        let device_static_pub = vec![3u8; 32];
        let transports = create_test_transports();
        let device_priv_key = vec![4u8; 32];

        let offer = RendezvousProtocol::create_offer_envelope(
            device_cert.clone(),
            &k_psk,
            &device_static_pub,
            transports,
            42,
            1000000,
            &device_priv_key,
        )
        .unwrap();

        let answer = RendezvousProtocol::create_answer_envelope(
            device_cert,
            &k_psk,
            &device_static_pub,
            &offer,
            0,
            43,
            1000000,
            &device_priv_key,
        );

        assert!(answer.is_ok());
        let answer_envelope = answer.unwrap();
        assert_eq!(answer_envelope.auth.kind, PayloadKind::Answer);
        assert_eq!(answer_envelope.auth.counter, 43);
        assert_eq!(answer_envelope.transport.selected_transport, Some(0));
    }

    #[test]
    fn test_answer_with_invalid_transport_index() {
        let device_cert = vec![1u8; 64];
        let k_psk = [2u8; 32];
        let device_static_pub = vec![3u8; 32];
        let transports = create_test_transports();
        let device_priv_key = vec![4u8; 32];

        let offer = RendezvousProtocol::create_offer_envelope(
            device_cert.clone(),
            &k_psk,
            &device_static_pub,
            transports,
            42,
            1000000,
            &device_priv_key,
        )
        .unwrap();

        let answer = RendezvousProtocol::create_answer_envelope(
            device_cert,
            &k_psk,
            &device_static_pub,
            &offer,
            99,
            43,
            1000000,
            &device_priv_key,
        );

        assert!(matches!(
            answer,
            Err(RendezvousError::NoCompatibleTransport)
        ));
    }

    #[test]
    fn test_verify_envelope_valid() {
        let device_cert = vec![1u8; 64];
        let k_psk = [2u8; 32];
        let device_static_pub = vec![3u8; 32];
        let transports = create_test_transports();
        let device_priv_key = vec![4u8; 32];

        let offer = RendezvousProtocol::create_offer_envelope(
            device_cert,
            &k_psk,
            &device_static_pub,
            transports,
            42,
            1000000,
            &device_priv_key,
        )
        .unwrap();

        let result =
            RendezvousProtocol::verify_envelope(&offer, &k_psk, &device_static_pub, 500000);
        assert!(result.is_ok());
    }

    #[test]
    fn test_verify_envelope_expired() {
        let device_cert = vec![1u8; 64];
        let k_psk = [2u8; 32];
        let device_static_pub = vec![3u8; 32];
        let transports = create_test_transports();
        let device_priv_key = vec![4u8; 32];

        let offer = RendezvousProtocol::create_offer_envelope(
            device_cert,
            &k_psk,
            &device_static_pub,
            transports,
            42,
            1000000,
            &device_priv_key,
        )
        .unwrap();

        let result =
            RendezvousProtocol::verify_envelope(&offer, &k_psk, &device_static_pub, 2000000);
        assert!(matches!(result, Err(RendezvousError::ExpiredOffer)));
    }

    #[test]
    fn test_verify_envelope_wrong_channel_binding() {
        let device_cert = vec![1u8; 64];
        let k_psk = [2u8; 32];
        let device_static_pub = vec![3u8; 32];
        let wrong_static_pub = vec![99u8; 32];
        let transports = create_test_transports();
        let device_priv_key = vec![4u8; 32];

        let offer = RendezvousProtocol::create_offer_envelope(
            device_cert,
            &k_psk,
            &device_static_pub,
            transports,
            42,
            1000000,
            &device_priv_key,
        )
        .unwrap();

        let result = RendezvousProtocol::verify_envelope(&offer, &k_psk, &wrong_static_pub, 500000);
        assert!(matches!(
            result,
            Err(RendezvousError::ChannelBindingMismatch)
        ));
    }

    #[test]
    fn test_handshake_transcript_binding() {
        let transcript = HandshakeTranscript::new(
            vec![1u8; 64],
            vec![2u8; 64],
            [3u8; 32],
            vec![4u8; 100],
            42,
            43,
        );

        let binding1 = transcript.compute_binding();
        let binding2 = transcript.compute_binding();

        assert_eq!(binding1, binding2);

        let modified_transcript = HandshakeTranscript::new(
            vec![1u8; 64],
            vec![2u8; 64],
            [3u8; 32],
            vec![4u8; 100],
            42,
            99,
        );

        let binding3 = modified_transcript.compute_binding();
        assert_ne!(binding1, binding3);
    }

    #[test]
    fn test_psk_handshake() {
        let k_psk = [5u8; 32];
        let device_static_priv = vec![6u8; 32];
        let device_static_pub = vec![7u8; 32];
        let peer_static_pub = vec![8u8; 32];

        let transcript = HandshakeTranscript::new(
            vec![1u8; 64],
            vec![2u8; 64],
            [3u8; 32],
            vec![4u8; 100],
            42,
            43,
        );

        let config = PskHandshakeConfig {
            k_psk,
            device_static_priv,
            device_static_pub,
            peer_static_pub,
            is_initiator: true,
        };

        let result = RendezvousProtocol::perform_psk_handshake(config, transcript);
        assert!(result.is_ok());

        let handshake_result = result.unwrap();
        assert_eq!(handshake_result.session_key.len(), 32);
        assert_eq!(handshake_result.transcript_binding.len(), 32);
    }

    #[test]
    fn test_channel_binding_computation() {
        let k_psk = [9u8; 32];
        let device_static_pub = vec![10u8; 32];

        let binding1 = AuthenticationPayload::compute_channel_binding(&k_psk, &device_static_pub);
        let binding2 = AuthenticationPayload::compute_channel_binding(&k_psk, &device_static_pub);

        assert_eq!(binding1, binding2);

        let different_pub = vec![11u8; 32];
        let binding3 = AuthenticationPayload::compute_channel_binding(&k_psk, &different_pub);

        assert_ne!(binding1, binding3);
    }

    #[test]
    fn test_transport_descriptors() {
        let quic = TransportDescriptor::quic("127.0.0.1:8080".to_string(), "h3".to_string());
        assert_eq!(quic.kind, TransportKind::Quic);
        assert_eq!(quic.metadata.get("addr").unwrap(), "127.0.0.1:8080");
        assert_eq!(quic.metadata.get("alpn").unwrap(), "h3");

        let tor = TransportDescriptor::tor("test.onion:443".to_string());
        assert_eq!(tor.kind, TransportKind::Tor);
        assert_eq!(tor.metadata.get("onion").unwrap(), "test.onion:443");

        let webrtc = TransportDescriptor::webrtc(
            "ufrag123".to_string(),
            "pwd456".to_string(),
            vec!["candidate1".to_string(), "candidate2".to_string()],
        );
        assert_eq!(webrtc.kind, TransportKind::WebRtc);
        assert_eq!(webrtc.metadata.get("ufrag").unwrap(), "ufrag123");
    }

    #[test]
    fn test_offer_without_transports() {
        let device_cert = vec![1u8; 64];
        let k_psk = [2u8; 32];
        let device_static_pub = vec![3u8; 32];
        let device_priv_key = vec![4u8; 32];

        let result = RendezvousProtocol::create_offer_envelope(
            device_cert,
            &k_psk,
            &device_static_pub,
            vec![],
            42,
            1000000,
            &device_priv_key,
        );

        assert!(matches!(
            result,
            Err(RendezvousError::NoTransportsAvailable)
        ));
    }
}
