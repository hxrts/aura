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
    /// Derive one hop's route-layer keys from a route secret seed and hop index.
    async fn derive_hop_key_material(
        &self,
        route_secret_seed: [u8; 32],
        hop_index: u16,
    ) -> Result<RouteHopKeyMaterial, RouteCryptoError>;

    /// Encrypt one hop layer with associated authenticated data and an explicit nonce.
    async fn encrypt_hop_layer(
        &self,
        key: [u8; 32],
        nonce: [u8; 12],
        aad: &[u8],
        plaintext: &[u8],
    ) -> Result<Vec<u8>, RouteCryptoError>;

    /// Decrypt one hop layer with associated authenticated data and an explicit nonce.
    async fn decrypt_hop_layer(
        &self,
        key: [u8; 32],
        nonce: [u8; 12],
        aad: &[u8],
        ciphertext: &[u8],
    ) -> Result<Vec<u8>, RouteCryptoError>;
}
