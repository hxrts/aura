//! Noise Protocol Effects
//!
//! This module defines the effect trait for Noise Protocol operations.
//! Implementations (like `aura-effects`) will wrap the `snow` crate.

use crate::AuraError;
use async_trait::async_trait;
use std::any::Any;

/// Error type for Noise operations
pub type NoiseError = AuraError;

/// Opaque wrapper for implementation-specific handshake state (e.g., snow::HandshakeState)
pub struct HandshakeState(pub Box<dyn Any + Send + Sync>);

/// Opaque wrapper for implementation-specific transport state (e.g., snow::TransportState)
pub struct TransportState(pub Box<dyn Any + Send + Sync>);

/// Parameters for initializing a Noise handshake
#[derive(Debug, Clone)]
pub struct NoiseParams {
    /// Local private key (32 bytes) - usually derived from Ed25519 identity
    pub local_private_key: [u8; 32],
    /// Remote public key (32 bytes) - usually derived from Ed25519 identity
    pub remote_public_key: [u8; 32],
    /// Pre-shared key (32 bytes)
    pub psk: [u8; 32],
    /// Whether this party is the initiator
    pub is_initiator: bool,
}

/// Noise Protocol Effects Trait
#[async_trait]
pub trait NoiseEffects: Send + Sync {
    /// Create a new handshake state machine (Initiator or Responder).
    /// Uses Noise_IKpsk2_25519_ChaChaPoly_BLAKE2s.
    async fn create_handshake_state(
        &self,
        params: NoiseParams,
    ) -> Result<HandshakeState, NoiseError>;

    /// Write a handshake message.
    /// Returns the encrypted payload and the updated state.
    async fn write_message(
        &self,
        state: HandshakeState,
        payload: &[u8],
    ) -> Result<(Vec<u8>, HandshakeState), NoiseError>;

    /// Read a handshake message.
    /// Returns the decrypted payload and the updated state.
    async fn read_message(
        &self,
        state: HandshakeState,
        message: &[u8],
    ) -> Result<(Vec<u8>, HandshakeState), NoiseError>;

    /// Transition to transport mode (Split).
    /// Returns the TransportState representing the secure channel pair.
    async fn into_transport_mode(
        &self,
        state: HandshakeState,
    ) -> Result<TransportState, NoiseError>;

    /// Encrypt a message using the TransportState.
    /// This is for post-handshake communication.
    /// Note: Typical Noise usage splits into two CipherStates (send/recv).
    /// This abstraction assumes the implementation handles the pair.
    async fn encrypt_transport_message(
        &self,
        state: &mut TransportState,
        payload: &[u8],
    ) -> Result<Vec<u8>, NoiseError>;

    /// Decrypt a message using the TransportState.
    async fn decrypt_transport_message(
        &self,
        state: &mut TransportState,
        message: &[u8],
    ) -> Result<Vec<u8>, NoiseError>;
}
