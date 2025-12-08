//! Packet Building and Cryptographic Operations
//!
//! This module provides packet construction and encryption for rendezvous
//! flooding. Packets are encrypted to the recipient's public key using
//! X25519 key agreement and ChaCha20-Poly1305 authenticated encryption.
//!
//! # Design Principles
//!
//! **Fixed-size packets**: All packets are padded to RENDEZVOUS_PACKET_SIZE
//! to prevent size-based fingerprinting.
//!
//! **Ephemeral keys**: Each packet uses a fresh ephemeral key, ensuring
//! unlinkability between packets from the same sender.
//!
//! **Authenticated encryption**: ChaCha20-Poly1305 provides both confidentiality
//! and authenticity, preventing tampering with packets.

use aura_core::{
    effects::{
        flood::{DecryptedRendezvous, FloodError, RendezvousPacket, RENDEZVOUS_PACKET_SIZE},
        CryptoEffects,
    },
    identifiers::AuthorityId,
};
use curve25519_dalek::{montgomery::MontgomeryPoint, scalar::Scalar};
use hkdf::Hkdf;
use sha2::Sha256;

/// Domain separator for rendezvous encryption key derivation.
const RENDEZVOUS_ENCRYPT_DOMAIN: &[u8] = b"AURA_RENDEZVOUS_ENCRYPT_v1";

/// Size of ChaCha20-Poly1305 authentication tag.
const AUTH_TAG_SIZE: usize = 16;

/// Size of the nonce for ChaCha20-Poly1305.
const NONCE_SIZE: usize = 12;

/// Decrypted payload from a rendezvous packet.
///
/// Contains the plaintext after successful decryption.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecryptedPayload {
    /// The sender's authority (revealed only to recipient).
    pub sender: AuthorityId,
    /// Protocol version.
    pub version: u8,
    /// The rendezvous payload data.
    pub payload: Vec<u8>,
}

impl From<DecryptedPayload> for DecryptedRendezvous {
    fn from(p: DecryptedPayload) -> Self {
        Self {
            sender: p.sender,
            version: p.version,
            payload: p.payload,
        }
    }
}

/// Packet builder for constructing encrypted rendezvous packets.
///
/// Uses the builder pattern to construct packets with all required
/// components before encryption.
///
/// # Example
///
/// ```ignore
/// let packet = PacketBuilder::new()
///     .with_sender(local_authority)
///     .with_payload(rendezvous_data)
///     .with_ttl(3)
///     .encrypt_to(&recipient_public_key, &crypto)?;
/// ```
#[derive(Debug, Clone)]
pub struct PacketBuilder {
    sender: Option<AuthorityId>,
    payload: Vec<u8>,
    version: u8,
    ttl: u8,
}

impl PacketBuilder {
    /// Create a new packet builder with default settings.
    pub fn new() -> Self {
        Self {
            sender: None,
            payload: Vec::new(),
            version: 1,
            ttl: 3,
        }
    }

    /// Set the sender authority.
    pub fn with_sender(mut self, sender: AuthorityId) -> Self {
        self.sender = Some(sender);
        self
    }

    /// Set the rendezvous payload data.
    pub fn with_payload(mut self, payload: Vec<u8>) -> Self {
        self.payload = payload;
        self
    }

    /// Set the protocol version.
    pub fn with_version(mut self, version: u8) -> Self {
        self.version = version;
        self
    }

    /// Set the TTL (time-to-live) for flood propagation.
    pub fn with_ttl(mut self, ttl: u8) -> Self {
        self.ttl = ttl;
        self
    }

    /// Encrypt the packet to the recipient's public key.
    ///
    /// Generates an ephemeral keypair, performs X25519 key agreement,
    /// derives a symmetric key, and encrypts the payload with ChaCha20-Poly1305.
    ///
    /// # Arguments
    /// * `recipient_public_key` - The recipient's X25519 public key (32 bytes)
    /// * `crypto` - Crypto effects for encryption and random number generation
    ///
    /// # Returns
    /// The encrypted packet ready for flooding
    pub async fn encrypt_to<C: CryptoEffects>(
        self,
        recipient_public_key: &[u8; 32],
        crypto: &C,
    ) -> Result<RendezvousPacket, FloodError> {
        let sender = self
            .sender
            .ok_or_else(|| FloodError::EncryptionError("sender not set".to_string()))?;

        // Build plaintext: version(1) + sender(16) + payload_len(2) + payload
        let mut plaintext = Vec::with_capacity(19 + self.payload.len());
        plaintext.push(self.version);
        plaintext.extend_from_slice(&sender.to_bytes()); // 16 bytes
        plaintext.extend_from_slice(&(self.payload.len() as u16).to_le_bytes());
        plaintext.extend_from_slice(&self.payload);

        // Encrypt
        let (ciphertext, ephemeral_key, nonce) =
            PacketCrypto::encrypt(&plaintext, recipient_public_key, crypto).await?;

        Ok(RendezvousPacket::new(
            ciphertext,
            ephemeral_key,
            self.ttl,
            nonce,
        ))
    }
}

impl Default for PacketBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Cryptographic operations for rendezvous packets.
///
/// Handles X25519 key agreement and ChaCha20-Poly1305 encryption/decryption.
pub struct PacketCrypto;

impl PacketCrypto {
    /// Encrypt plaintext to a recipient's public key.
    ///
    /// Uses X25519 key agreement with an ephemeral keypair, then ChaCha20-Poly1305
    /// for authenticated encryption.
    ///
    /// # Arguments
    /// * `plaintext` - Data to encrypt
    /// * `recipient_public_key` - Recipient's X25519 public key
    /// * `crypto` - Crypto effects for random number generation and encryption
    ///
    /// # Returns
    /// Tuple of (ciphertext, ephemeral_public_key, nonce)
    pub async fn encrypt<C: CryptoEffects>(
        plaintext: &[u8],
        recipient_public_key: &[u8; 32],
        crypto: &C,
    ) -> Result<(Vec<u8>, [u8; 32], [u8; 16]), FloodError> {
        // Generate ephemeral X25519 keypair
        let ephemeral_scalar_bytes = crypto.random_bytes_32().await;
        let ephemeral_scalar = Scalar::from_bytes_mod_order(ephemeral_scalar_bytes);
        let ephemeral_public = MontgomeryPoint::mul_base(&ephemeral_scalar);

        // Perform X25519 key agreement
        let recipient_point = MontgomeryPoint(*recipient_public_key);
        let shared_secret = ephemeral_scalar * recipient_point;

        // Derive symmetric key using HKDF
        let symmetric_key = Self::derive_key(
            shared_secret.as_bytes(),
            ephemeral_public.as_bytes(),
            recipient_public_key,
        )?;

        // Generate nonce for ChaCha20
        let nonce_bytes = crypto.random_bytes(NONCE_SIZE).await;
        let mut nonce = [0u8; NONCE_SIZE];
        nonce.copy_from_slice(&nonce_bytes);

        // Pad plaintext to fixed size (accounting for auth tag)
        let max_plaintext_size = RENDEZVOUS_PACKET_SIZE - AUTH_TAG_SIZE;
        if plaintext.len() > max_plaintext_size {
            return Err(FloodError::PacketTooLarge {
                size: plaintext.len(),
                max_size: max_plaintext_size,
            });
        }

        let mut padded = vec![0u8; max_plaintext_size];
        padded[..plaintext.len()].copy_from_slice(plaintext);

        // Encrypt with ChaCha20-Poly1305
        let ciphertext = crypto
            .chacha20_encrypt(&padded, &symmetric_key, &nonce)
            .await
            .map_err(|e| FloodError::EncryptionError(e.to_string()))?;

        // Generate packet nonce (for deduplication, different from encryption nonce)
        let packet_nonce_bytes = crypto.random_bytes(16).await;
        let mut packet_nonce = [0u8; 16];
        packet_nonce.copy_from_slice(&packet_nonce_bytes);

        // Prepend encryption nonce to ciphertext
        let mut result = Vec::with_capacity(NONCE_SIZE + ciphertext.len());
        result.extend_from_slice(&nonce);
        result.extend_from_slice(&ciphertext);

        Ok((result, *ephemeral_public.as_bytes(), packet_nonce))
    }

    /// Decrypt a rendezvous packet using our private key.
    ///
    /// # Arguments
    /// * `packet` - The encrypted packet
    /// * `our_private_key` - Our X25519 private key (scalar bytes)
    /// * `crypto` - Crypto effects for decryption
    ///
    /// # Returns
    /// The decrypted payload, or None if decryption failed
    pub async fn decrypt<C: CryptoEffects>(
        packet: &RendezvousPacket,
        our_private_key: &[u8; 32],
        crypto: &C,
    ) -> Option<DecryptedPayload> {
        // Extract nonce from ciphertext
        if packet.ciphertext.len() < NONCE_SIZE + AUTH_TAG_SIZE {
            return None;
        }

        let nonce: [u8; NONCE_SIZE] = packet.ciphertext[..NONCE_SIZE].try_into().ok()?;
        let ciphertext = &packet.ciphertext[NONCE_SIZE..];

        // Perform X25519 key agreement
        let our_scalar = Scalar::from_bytes_mod_order(*our_private_key);
        let ephemeral_point = MontgomeryPoint(packet.ephemeral_key);
        let shared_secret = our_scalar * ephemeral_point;

        // Derive symmetric key
        let our_public = MontgomeryPoint::mul_base(&our_scalar);
        let symmetric_key = Self::derive_key(
            shared_secret.as_bytes(),
            &packet.ephemeral_key,
            our_public.as_bytes(),
        )
        .ok()?;

        // Decrypt
        let padded_plaintext = crypto
            .chacha20_decrypt(ciphertext, &symmetric_key, &nonce)
            .await
            .ok()?;

        // Parse plaintext: version(1) + sender(16) + payload_len(2) + payload
        if padded_plaintext.len() < 19 {
            return None;
        }

        let version = padded_plaintext[0];
        let sender_bytes: [u8; 16] = padded_plaintext[1..17].try_into().ok()?;
        let sender = AuthorityId::from_uuid(uuid::Uuid::from_bytes(sender_bytes));
        let payload_len = u16::from_le_bytes(padded_plaintext[17..19].try_into().ok()?) as usize;

        if 19 + payload_len > padded_plaintext.len() {
            return None;
        }

        let payload = padded_plaintext[19..19 + payload_len].to_vec();

        Some(DecryptedPayload {
            sender,
            version,
            payload,
        })
    }

    /// Try to decrypt a packet, returning None if we're not the recipient.
    ///
    /// This is the main entry point for receiving flooded packets.
    pub async fn try_decrypt<C: CryptoEffects>(
        packet: &RendezvousPacket,
        our_private_key: &[u8; 32],
        crypto: &C,
    ) -> Option<DecryptedPayload> {
        Self::decrypt(packet, our_private_key, crypto).await
    }

    /// Derive a symmetric key from the shared secret.
    fn derive_key(
        shared_secret: &[u8; 32],
        ephemeral_public: &[u8; 32],
        recipient_public: &[u8; 32],
    ) -> Result<[u8; 32], FloodError> {
        // Build info string with both public keys for domain separation
        let mut info = Vec::with_capacity(64);
        info.extend_from_slice(ephemeral_public);
        info.extend_from_slice(recipient_public);

        let hkdf = Hkdf::<Sha256>::new(Some(RENDEZVOUS_ENCRYPT_DOMAIN), shared_secret);
        let mut output = [0u8; 32];
        hkdf.expand(&info, &mut output)
            .map_err(|_| FloodError::EncryptionError("HKDF expansion failed".to_string()))?;

        Ok(output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Mock crypto for testing
    struct MockCrypto {
        random_counter: std::sync::atomic::AtomicU64,
    }

    impl MockCrypto {
        fn new() -> Self {
            Self {
                random_counter: std::sync::atomic::AtomicU64::new(0),
            }
        }

        fn next_seed(&self) -> u64 {
            self.random_counter
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
        }
    }

    #[async_trait::async_trait]
    impl aura_core::effects::RandomEffects for MockCrypto {
        async fn random_bytes(&self, len: usize) -> Vec<u8> {
            let seed = self.next_seed();
            let mut bytes = vec![0u8; len];
            for (i, b) in bytes.iter_mut().enumerate() {
                *b = ((seed + i as u64) % 256) as u8;
            }
            bytes
        }

        async fn random_bytes_32(&self) -> [u8; 32] {
            let seed = self.next_seed();
            let mut bytes = [0u8; 32];
            for (i, b) in bytes.iter_mut().enumerate() {
                *b = ((seed + i as u64) % 256) as u8;
            }
            bytes
        }

        async fn random_range(&self, min: u64, max: u64) -> u64 {
            let seed = self.next_seed();
            min + (seed % (max - min + 1))
        }

        async fn random_u64(&self) -> u64 {
            self.next_seed()
        }

        async fn random_uuid(&self) -> uuid::Uuid {
            let bytes = self.random_bytes(16).await;
            let mut uuid_bytes = [0u8; 16];
            uuid_bytes.copy_from_slice(&bytes);
            uuid::Uuid::from_bytes(uuid_bytes)
        }
    }

    #[async_trait::async_trait]
    impl CryptoEffects for MockCrypto {
        async fn hkdf_derive(
            &self,
            ikm: &[u8],
            salt: &[u8],
            info: &[u8],
            output_len: usize,
        ) -> Result<Vec<u8>, aura_core::AuraError> {
            let hkdf = Hkdf::<Sha256>::new(Some(salt), ikm);
            let mut output = vec![0u8; output_len];
            hkdf.expand(info, &mut output)
                .map_err(|_| aura_core::AuraError::crypto("HKDF failed"))?;
            Ok(output)
        }

        async fn derive_key(
            &self,
            _master_key: &[u8],
            _context: &aura_core::effects::crypto::KeyDerivationContext,
        ) -> Result<Vec<u8>, aura_core::AuraError> {
            Ok(vec![0u8; 32])
        }

        async fn ed25519_generate_keypair(
            &self,
        ) -> Result<(Vec<u8>, Vec<u8>), aura_core::AuraError> {
            Ok((vec![0u8; 32], vec![0u8; 32]))
        }

        async fn ed25519_sign(
            &self,
            _message: &[u8],
            _private_key: &[u8],
        ) -> Result<Vec<u8>, aura_core::AuraError> {
            Ok(vec![0u8; 64])
        }

        async fn ed25519_verify(
            &self,
            _message: &[u8],
            _signature: &[u8],
            _public_key: &[u8],
        ) -> Result<bool, aura_core::AuraError> {
            Ok(true)
        }

        async fn frost_generate_keys(
            &self,
            _threshold: u16,
            _max_signers: u16,
        ) -> Result<aura_core::effects::crypto::FrostKeyGenResult, aura_core::AuraError> {
            Ok(aura_core::effects::crypto::FrostKeyGenResult {
                key_packages: vec![],
                public_key_package: vec![],
            })
        }

        async fn frost_generate_nonces(
            &self,
            _key_package: &[u8],
        ) -> Result<Vec<u8>, aura_core::AuraError> {
            Ok(vec![])
        }

        async fn frost_create_signing_package(
            &self,
            _message: &[u8],
            _nonces: &[Vec<u8>],
            _participants: &[u16],
            _public_key_package: &[u8],
        ) -> Result<aura_core::effects::crypto::FrostSigningPackage, aura_core::AuraError> {
            Ok(aura_core::effects::crypto::FrostSigningPackage {
                message: vec![],
                package: vec![],
                participants: vec![],
                public_key_package: vec![],
            })
        }

        async fn frost_sign_share(
            &self,
            _signing_package: &aura_core::effects::crypto::FrostSigningPackage,
            _key_share: &[u8],
            _nonces: &[u8],
        ) -> Result<Vec<u8>, aura_core::AuraError> {
            Ok(vec![])
        }

        async fn frost_aggregate_signatures(
            &self,
            _signing_package: &aura_core::effects::crypto::FrostSigningPackage,
            _signature_shares: &[Vec<u8>],
        ) -> Result<Vec<u8>, aura_core::AuraError> {
            Ok(vec![])
        }

        async fn frost_verify(
            &self,
            _message: &[u8],
            _signature: &[u8],
            _group_public_key: &[u8],
        ) -> Result<bool, aura_core::AuraError> {
            Ok(true)
        }

        async fn ed25519_public_key(
            &self,
            _private_key: &[u8],
        ) -> Result<Vec<u8>, aura_core::AuraError> {
            Ok(vec![0u8; 32])
        }

        async fn chacha20_encrypt(
            &self,
            plaintext: &[u8],
            _key: &[u8; 32],
            _nonce: &[u8; 12],
        ) -> Result<Vec<u8>, aura_core::AuraError> {
            // Simple mock: just copy plaintext and add auth tag
            let mut result = plaintext.to_vec();
            result.extend_from_slice(&[0u8; AUTH_TAG_SIZE]);
            Ok(result)
        }

        async fn chacha20_decrypt(
            &self,
            ciphertext: &[u8],
            _key: &[u8; 32],
            _nonce: &[u8; 12],
        ) -> Result<Vec<u8>, aura_core::AuraError> {
            // Simple mock: just strip auth tag
            if ciphertext.len() < AUTH_TAG_SIZE {
                return Err(aura_core::AuraError::crypto("too short"));
            }
            Ok(ciphertext[..ciphertext.len() - AUTH_TAG_SIZE].to_vec())
        }

        async fn aes_gcm_encrypt(
            &self,
            plaintext: &[u8],
            _key: &[u8; 32],
            _nonce: &[u8; 12],
        ) -> Result<Vec<u8>, aura_core::AuraError> {
            Ok(plaintext.to_vec())
        }

        async fn aes_gcm_decrypt(
            &self,
            ciphertext: &[u8],
            _key: &[u8; 32],
            _nonce: &[u8; 12],
        ) -> Result<Vec<u8>, aura_core::AuraError> {
            Ok(ciphertext.to_vec())
        }

        async fn frost_rotate_keys(
            &self,
            _old_shares: &[Vec<u8>],
            _old_threshold: u16,
            _new_threshold: u16,
            _new_max_signers: u16,
        ) -> Result<aura_core::effects::crypto::FrostKeyGenResult, aura_core::AuraError> {
            Ok(aura_core::effects::crypto::FrostKeyGenResult {
                key_packages: vec![],
                public_key_package: vec![],
            })
        }

        fn is_simulated(&self) -> bool {
            true
        }

        fn crypto_capabilities(&self) -> Vec<String> {
            vec!["mock".to_string()]
        }

        fn constant_time_eq(&self, a: &[u8], b: &[u8]) -> bool {
            a == b
        }

        fn secure_zero(&self, data: &mut [u8]) {
            for b in data.iter_mut() {
                *b = 0;
            }
        }

        async fn generate_signing_keys(
            &self,
            _threshold: u16,
            _max_signers: u16,
        ) -> Result<aura_core::effects::crypto::SigningKeyGenResult, aura_core::AuraError> {
            Ok(aura_core::effects::crypto::SigningKeyGenResult {
                key_packages: vec![vec![0u8; 32]],
                public_key_package: vec![0u8; 32],
                mode: aura_core::crypto::single_signer::SigningMode::SingleSigner,
            })
        }

        async fn sign_with_key(
            &self,
            _message: &[u8],
            _key_package: &[u8],
            _mode: aura_core::crypto::single_signer::SigningMode,
        ) -> Result<Vec<u8>, aura_core::AuraError> {
            Ok(vec![0u8; 64])
        }

        async fn verify_signature(
            &self,
            _message: &[u8],
            _signature: &[u8],
            _public_key_package: &[u8],
            _mode: aura_core::crypto::single_signer::SigningMode,
        ) -> Result<bool, aura_core::AuraError> {
            Ok(true)
        }
    }

    #[tokio::test]
    async fn test_packet_builder_basic() {
        let crypto = MockCrypto::new();
        let sender = AuthorityId::new_from_entropy([1u8; 32]);
        let recipient_key = [2u8; 32];

        let packet = PacketBuilder::new()
            .with_sender(sender)
            .with_payload(b"hello world".to_vec())
            .with_ttl(3)
            .encrypt_to(&recipient_key, &crypto)
            .await
            .unwrap();

        assert_eq!(packet.ttl, 3);
        assert!(!packet.ciphertext.is_empty());
    }

    #[tokio::test]
    async fn test_packet_builder_requires_sender() {
        let crypto = MockCrypto::new();
        let recipient_key = [2u8; 32];

        let result = PacketBuilder::new()
            .with_payload(b"hello".to_vec())
            .encrypt_to(&recipient_key, &crypto)
            .await;

        assert!(matches!(result, Err(FloodError::EncryptionError(_))));
    }

    #[tokio::test]
    async fn test_packet_too_large() {
        let crypto = MockCrypto::new();
        let sender = AuthorityId::new_from_entropy([1u8; 32]);
        let recipient_key = [2u8; 32];

        // Create payload that exceeds max size
        let large_payload = vec![0u8; RENDEZVOUS_PACKET_SIZE];

        let result = PacketBuilder::new()
            .with_sender(sender)
            .with_payload(large_payload)
            .encrypt_to(&recipient_key, &crypto)
            .await;

        assert!(matches!(result, Err(FloodError::PacketTooLarge { .. })));
    }

    #[test]
    fn test_derive_key_deterministic() {
        let shared_secret = [1u8; 32];
        let ephemeral = [2u8; 32];
        let recipient = [3u8; 32];

        let key1 = PacketCrypto::derive_key(&shared_secret, &ephemeral, &recipient).unwrap();
        let key2 = PacketCrypto::derive_key(&shared_secret, &ephemeral, &recipient).unwrap();

        assert_eq!(key1, key2);
    }

    #[test]
    fn test_derive_key_different_inputs() {
        let shared_secret = [1u8; 32];
        let ephemeral1 = [2u8; 32];
        let ephemeral2 = [3u8; 32];
        let recipient = [4u8; 32];

        let key1 = PacketCrypto::derive_key(&shared_secret, &ephemeral1, &recipient).unwrap();
        let key2 = PacketCrypto::derive_key(&shared_secret, &ephemeral2, &recipient).unwrap();

        assert_ne!(key1, key2);
    }

    #[test]
    fn test_decrypted_payload_into_decrypted_rendezvous() {
        let payload = DecryptedPayload {
            sender: AuthorityId::new_from_entropy([1u8; 32]),
            version: 1,
            payload: b"test".to_vec(),
        };

        let rendezvous: DecryptedRendezvous = payload.into();
        assert_eq!(rendezvous.version, 1);
        assert_eq!(rendezvous.payload, b"test".to_vec());
    }
}
