//! Route-layer hop crypto effects.
//!
//! This trait defines the stateless cryptographic primitives used by the
//! anonymous route layer. It is distinct from `NoiseEffects`. `NoiseEffects`
//! protects one adjacent transport hop. `RouteCryptoEffects` derives and uses
//! per-hop path-layer keys for multi-hop route setup and peel processing.

use crate::AuraError;
use async_trait::async_trait;

/// Error type for route-layer cryptographic operations.
pub type RouteCryptoError = AuraError;

/// Per-hop route-layer key material.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouteHopKeyMaterial {
    /// Forward key used while advancing toward the destination.
    pub forward_key: [u8; 32],
    /// Backward key used while returning over a reply block or reply path.
    pub backward_key: [u8; 32],
    /// Replay-window binding for this hop.
    pub replay_window_id: [u8; 32],
}

/// Stateless route-layer hop cryptography.
#[async_trait]
pub trait RouteCryptoEffects: Send + Sync {
    /// Derive a route-layer public key from route-layer private key material.
    async fn route_public_key(
        &self,
        route_private_key: [u8; 32],
    ) -> Result<[u8; 32], RouteCryptoError>;

    /// Derive a route setup peel key from local private and peer public key material.
    async fn derive_route_setup_key(
        &self,
        local_private_key: [u8; 32],
        peer_public_key: [u8; 32],
        context: &[u8],
    ) -> Result<[u8; 32], RouteCryptoError>;

    /// Derive one hop's route-layer keys from a route secret seed and hop index.
    async fn derive_hop_key_material(
        &self,
        route_secret_seed: [u8; 32],
        hop_index: u16,
    ) -> Result<RouteHopKeyMaterial, RouteCryptoError>;

    /// Encrypt one hop layer with associated authenticated data and an explicit nonce.
    ///
    /// The caller MUST supply a nonce that is unique for this AEAD key and
    /// direction. Reusing a nonce with the same hop key can break
    /// confidentiality and integrity. Fresh random nonces, monotonic counters,
    /// or deterministic nonces derived from per-message unique key material are
    /// acceptable; deterministic per-hop constants are not sufficient when a
    /// hop key can protect more than one message.
    async fn encrypt_hop_layer(
        &self,
        key: [u8; 32],
        nonce: [u8; 12],
        aad: &[u8],
        plaintext: &[u8],
    ) -> Result<Vec<u8>, RouteCryptoError>;

    /// Decrypt one hop layer with associated authenticated data and an explicit nonce.
    ///
    /// Decryption must use the nonce carried or otherwise authenticated by the
    /// wire format for the corresponding ciphertext. It must not invent a
    /// fallback nonce when decoding fails.
    async fn decrypt_hop_layer(
        &self,
        key: [u8; 32],
        nonce: [u8; 12],
        aad: &[u8],
        ciphertext: &[u8],
    ) -> Result<Vec<u8>, RouteCryptoError>;
}
