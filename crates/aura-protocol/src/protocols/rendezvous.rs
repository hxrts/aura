//! Rendezvous Protocol Implementation
//!
//! Implements the SSB Offer/Answer exchange for establishing direct connections
//! between devices from different accounts. The protocol flow is:
//!
//! 1. Device A publishes Offer envelope with available transports
//! 2. Device B recognizes Offer, selects transport, publishes Answer
//! 3. Both devices perform PSK-bound handshake on selected transport
//!
//! Reference: docs/051_rendezvous.md Section 4.3

// Import message types from aura-messages
use aura_messages::protocol::{
    AuthenticationPayload, HandshakeResult, HandshakeTranscript, PayloadKind, PskHandshakeConfig,
    RendezvousEnvelope, TransportDescriptor, TransportOfferPayload,
};

/// Error types for rendezvous protocol
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

/// Rendezvous protocol implementation
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

    pub fn verify_offer_envelope(
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

        // Additional signature verification would go here
        Ok(())
    }

    pub fn verify_answer_envelope(
        answer: &RendezvousEnvelope,
        original_offer: &RendezvousEnvelope,
        k_psk: &[u8; 32],
        peer_static_pub: &[u8],
        current_time: u64,
    ) -> Result<()> {
        if answer.auth.expires < current_time {
            return Err(RendezvousError::ExpiredOffer);
        }

        let expected_binding =
            AuthenticationPayload::compute_channel_binding(k_psk, peer_static_pub);

        if answer.auth.channel_binding != expected_binding {
            return Err(RendezvousError::ChannelBindingMismatch);
        }

        // Verify that the selected transport index is valid
        if let Some(selected_idx) = answer.transport.selected_transport {
            if (selected_idx as usize) >= original_offer.transport.transports.len() {
                return Err(RendezvousError::NoCompatibleTransport);
            }
        } else {
            return Err(RendezvousError::NoCompatibleTransport);
        }

        Ok(())
    }

    pub fn perform_psk_handshake(
        config: &PskHandshakeConfig,
        peer_envelope: &RendezvousEnvelope,
    ) -> Result<HandshakeResult> {
        // Create transcript for verification
        let transcript = HandshakeTranscript::new(
            config.local_device_cert.clone(),
            peer_envelope.auth.device_cert.clone(),
            peer_envelope.auth.channel_binding,
            serde_json::to_vec(&config.transport_descriptor)
                .map_err(|e| RendezvousError::HandshakeFailed(e.to_string()))?,
            peer_envelope.auth.counter,
            peer_envelope.auth.counter + 1, // Assumed counter increment
        );

        let transcript_binding = transcript.compute_binding();

        // Verify transcript binding
        if transcript_binding != peer_envelope.auth.channel_binding {
            return Err(RendezvousError::TranscriptBindingMismatch);
        }

        Ok(HandshakeResult {
            success: true,
            transcript: Some(transcript),
            session_key: Some(config.k_psk), // Simplified - real implementation would derive new key
            error_message: None,
        })
    }

    fn sign_payload(
        kind: &PayloadKind,
        channel_binding: &[u8; 32],
        counter: u32,
        expires: u64,
        device_priv_key: &[u8],
    ) -> Vec<u8> {
        // Create message to sign
        let mut message = Vec::new();
        message.extend_from_slice(&[*kind as u8]);
        message.extend_from_slice(channel_binding);
        message.extend_from_slice(&counter.to_le_bytes());
        message.extend_from_slice(&expires.to_le_bytes());

        // Simplified signing - real implementation would use proper Ed25519 signing
        use aura_crypto::blake3_hash;
        let mut signing_input = Vec::new();
        signing_input.extend_from_slice(device_priv_key);
        signing_input.extend_from_slice(&message);
        blake3_hash(&signing_input).to_vec()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_offer_envelope() {
        let device_cert = vec![1, 2, 3, 4];
        let k_psk = [42u8; 32];
        let device_static_pub = [1u8; 32];
        let transports = vec![TransportDescriptor::quic(
            "127.0.0.1:8080".to_string(),
            "aura/1".to_string(),
        )];
        let counter = 1;
        let expires = 1000000000;
        let device_priv_key = [2u8; 32];

        let result = RendezvousProtocol::create_offer_envelope(
            device_cert,
            &k_psk,
            &device_static_pub,
            transports,
            counter,
            expires,
            &device_priv_key,
        );

        assert!(result.is_ok());
        let envelope = result.unwrap();
        assert_eq!(envelope.auth.kind, PayloadKind::Offer);
        assert_eq!(envelope.transport.transports.len(), 1);
    }
}
