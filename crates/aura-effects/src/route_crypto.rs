//! Route-layer hop crypto implementation.
#![allow(clippy::disallowed_types)]

use async_trait::async_trait;
use aura_core::crypto::kdf;
use aura_core::effects::{RouteCryptoEffects, RouteCryptoError, RouteHopKeyMaterial};
use aura_core::AuraError;
use chacha20poly1305::aead::{Aead, Payload};
use chacha20poly1305::{ChaCha20Poly1305, KeyInit, Nonce};
use curve25519_dalek::montgomery::MontgomeryPoint;
use curve25519_dalek::scalar::Scalar;

/// Stateless production route-layer crypto handler.
#[derive(Debug, Default, Clone)]
pub struct RealRouteCryptoHandler;

impl RealRouteCryptoHandler {
    /// Create a new route-layer crypto handler.
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl RouteCryptoEffects for RealRouteCryptoHandler {
    async fn route_public_key(
        &self,
        route_private_key: [u8; 32],
    ) -> Result<[u8; 32], RouteCryptoError> {
        let scalar = route_scalar(route_private_key);
        Ok(MontgomeryPoint::mul_base(&scalar).to_bytes())
    }

    async fn derive_route_setup_key(
        &self,
        local_private_key: [u8; 32],
        peer_public_key: [u8; 32],
        context: &[u8],
    ) -> Result<[u8; 32], RouteCryptoError> {
        let scalar = route_scalar(local_private_key);
        let peer = MontgomeryPoint(peer_public_key);
        let shared_secret = scalar * peer;
        let shared_bytes = shared_secret.to_bytes();
        if shared_bytes == [0u8; 32] {
            return Err(AuraError::crypto("invalid route setup shared secret"));
        }
        kdf::derive_key::<32>(&shared_bytes, b"aura.route.v1", context)
            .map_err(|_| AuraError::crypto("derive route setup peel key"))
    }

    async fn derive_hop_key_material(
        &self,
        route_secret_seed: [u8; 32],
        hop_index: u16,
    ) -> Result<RouteHopKeyMaterial, RouteCryptoError> {
        let forward_key = kdf::derive_key::<32>(
            &route_secret_seed,
            b"aura.route.v1",
            format!("aura.route.forward.{hop_index}").as_bytes(),
        )
        .map_err(|_| AuraError::crypto("derive forward hop key material"))?;
        let backward_key = kdf::derive_key::<32>(
            &route_secret_seed,
            b"aura.route.v1",
            format!("aura.route.backward.{hop_index}").as_bytes(),
        )
        .map_err(|_| AuraError::crypto("derive backward hop key material"))?;
        let replay_window_id = kdf::derive_key::<32>(
            &route_secret_seed,
            b"aura.route.v1",
            format!("aura.route.replay.{hop_index}").as_bytes(),
        )
        .map_err(|_| AuraError::crypto("derive replay-window binding"))?;

        Ok(RouteHopKeyMaterial {
            forward_key,
            backward_key,
            replay_window_id,
        })
    }

    async fn encrypt_hop_layer(
        &self,
        key: [u8; 32],
        nonce: [u8; 12],
        aad: &[u8],
        plaintext: &[u8],
    ) -> Result<Vec<u8>, RouteCryptoError> {
        let cipher = ChaCha20Poly1305::new((&key).into());
        cipher
            .encrypt(
                Nonce::from_slice(&nonce),
                Payload {
                    msg: plaintext,
                    aad,
                },
            )
            .map_err(|_| AuraError::crypto("encrypt route hop layer"))
    }

    async fn decrypt_hop_layer(
        &self,
        key: [u8; 32],
        nonce: [u8; 12],
        aad: &[u8],
        ciphertext: &[u8],
    ) -> Result<Vec<u8>, RouteCryptoError> {
        let cipher = ChaCha20Poly1305::new((&key).into());
        cipher
            .decrypt(
                Nonce::from_slice(&nonce),
                Payload {
                    msg: ciphertext,
                    aad,
                },
            )
            .map_err(|_| AuraError::crypto("decrypt route hop layer"))
    }
}

fn route_scalar(mut private_key: [u8; 32]) -> Scalar {
    private_key[0] &= 248;
    private_key[31] &= 127;
    private_key[31] |= 64;
    Scalar::from_bytes_mod_order(private_key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn derives_distinct_forward_backward_and_replay_material_per_hop(
    ) -> Result<(), RouteCryptoError> {
        let handler = RealRouteCryptoHandler::new();
        let first = handler.derive_hop_key_material([7u8; 32], 0).await?;
        let second = handler.derive_hop_key_material([7u8; 32], 1).await?;

        assert_ne!(first.forward_key, first.backward_key);
        assert_ne!(first.replay_window_id, first.forward_key);
        assert_ne!(first.forward_key, second.forward_key);
        assert_ne!(first.backward_key, second.backward_key);
        Ok(())
    }

    #[tokio::test]
    async fn derives_symmetric_route_setup_key_from_private_and_public_keys(
    ) -> Result<(), RouteCryptoError> {
        let handler = RealRouteCryptoHandler::new();
        let initiator_private = [9u8; 32];
        let hop_private = [42u8; 32];
        let initiator_public = handler.route_public_key(initiator_private).await?;
        let hop_public = handler.route_public_key(hop_private).await?;
        let context = b"aura.route.setup.test";

        let initiator_key = handler
            .derive_route_setup_key(initiator_private, hop_public, context)
            .await?;
        let hop_key = handler
            .derive_route_setup_key(hop_private, initiator_public, context)
            .await?;

        assert_eq!(initiator_key, hop_key);
        Ok(())
    }

    #[tokio::test]
    async fn route_setup_key_changes_with_initiator_ephemeral_key() -> Result<(), RouteCryptoError>
    {
        let handler = RealRouteCryptoHandler::new();
        let hop_private = [42u8; 32];
        let hop_public = handler.route_public_key(hop_private).await?;
        let first = handler
            .derive_route_setup_key([9u8; 32], hop_public, b"aura.route.setup.test")
            .await?;
        let second = handler
            .derive_route_setup_key([10u8; 32], hop_public, b"aura.route.setup.test")
            .await?;

        assert_ne!(first, second);
        Ok(())
    }

    #[tokio::test]
    async fn encrypts_and_decrypts_one_hop_layer_with_aad() -> Result<(), RouteCryptoError> {
        let handler = RealRouteCryptoHandler::new();
        let material = handler.derive_hop_key_material([9u8; 32], 2).await?;
        let nonce = [3u8; 12];
        let aad = b"aura.route.hop.2";
        let plaintext = b"opaque-next-hop-layer";

        let ciphertext = handler
            .encrypt_hop_layer(material.forward_key, nonce, aad, plaintext)
            .await?;
        let decrypted = handler
            .decrypt_hop_layer(material.forward_key, nonce, aad, &ciphertext)
            .await?;

        assert_eq!(decrypted, plaintext);
        Ok(())
    }

    #[tokio::test]
    async fn aad_or_key_mismatch_rejects_decryption() -> Result<(), RouteCryptoError> {
        let handler = RealRouteCryptoHandler::new();
        let material = handler.derive_hop_key_material([11u8; 32], 4).await?;
        let wrong = handler.derive_hop_key_material([12u8; 32], 4).await?;
        let nonce = [5u8; 12];

        let ciphertext = handler
            .encrypt_hop_layer(material.forward_key, nonce, b"aad", b"payload")
            .await?;

        assert!(handler
            .decrypt_hop_layer(wrong.forward_key, nonce, b"aad", &ciphertext)
            .await
            .is_err());
        assert!(handler
            .decrypt_hop_layer(material.forward_key, nonce, b"wrong", &ciphertext)
            .await
            .is_err());
        Ok(())
    }
}
