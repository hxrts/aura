// HPKE (Hybrid Public Key Encryption) primitives
//
// Reference: 080_architecture_protocol_integration.md
// - Part 4: P2P Resharing (sub-share encryption)
// - Part 2: Recovery Protocol (guardian share encryption)
//
// This module provides authenticated encryption for:
// 1. Resharing sub-shares: HPKE::encrypt(sub_share, recipient_pk)
// 2. Guardian shares: HPKE::encrypt(share, new_device_pk) with associated data
// 3. Decryption: HPKE::decrypt(ciphertext, device_secret)

use crate::{CryptoError, Result};
use hpke::{
    aead::ChaCha20Poly1305, kdf::HkdfSha256, kem::DhP256HkdfSha256, Deserializable, Kem, OpModeR,
    OpModeS, Serializable,
};
use rand::{CryptoRng, RngCore};
use zeroize::{Zeroize, ZeroizeOnDrop};

/// HPKE configuration using NIST P-256, HKDF-SHA256, and ChaCha20-Poly1305
///
/// This is a conservative, widely-supported configuration suitable for
/// guardian share and sub-share encryption
type AeadAlg = ChaCha20Poly1305;
type KdfAlg = HkdfSha256;
type KemAlg = DhP256HkdfSha256;

/// Public key for HPKE encryption
#[derive(Clone)]
pub struct HpkePublicKey {
    inner: <KemAlg as Kem>::PublicKey,
}

impl HpkePublicKey {
    /// Serialize public key to bytes
    pub fn to_bytes(&self) -> Vec<u8> {
        self.inner.to_bytes().to_vec()
    }

    /// Deserialize public key from bytes
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        let inner = <KemAlg as Kem>::PublicKey::from_bytes(bytes)
            .map_err(|e| CryptoError::InvalidKey(format!("Invalid HPKE public key: {:?}", e)))?;
        Ok(HpkePublicKey { inner })
    }
}

/// Private key for HPKE decryption
#[derive(Clone, Zeroize, ZeroizeOnDrop)]
pub struct HpkePrivateKey {
    #[zeroize(skip)]
    inner: <KemAlg as Kem>::PrivateKey,
}

impl HpkePrivateKey {
    /// Serialize private key to bytes (for secure storage)
    pub fn to_bytes(&self) -> Vec<u8> {
        self.inner.to_bytes().to_vec()
    }

    /// Deserialize private key from bytes
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        let inner = <KemAlg as Kem>::PrivateKey::from_bytes(bytes)
            .map_err(|e| CryptoError::InvalidKey(format!("Invalid HPKE private key: {:?}", e)))?;
        Ok(HpkePrivateKey { inner })
    }
}

/// HPKE key pair
pub struct HpkeKeyPair {
    /// Public key for encryption
    pub public_key: HpkePublicKey,
    /// Private key for decryption
    pub private_key: HpkePrivateKey,
}

impl HpkeKeyPair {
    /// Generate a new HPKE key pair
    pub fn generate<R: RngCore + CryptoRng>(rng: &mut R) -> Self {
        let (private_key, public_key) = KemAlg::gen_keypair(rng);

        HpkeKeyPair {
            public_key: HpkePublicKey { inner: public_key },
            private_key: HpkePrivateKey { inner: private_key },
        }
    }
}

/// Encrypted message with encapsulated key
#[derive(Clone)]
pub struct HpkeCiphertext {
    /// Encapsulated key (ephemeral public key)
    pub encapped_key: Vec<u8>,
    /// Encrypted payload
    pub ciphertext: Vec<u8>,
}

impl HpkeCiphertext {
    /// Serialize to bytes (encapped_key_len || encapped_key || ciphertext)
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&(self.encapped_key.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&self.encapped_key);
        bytes.extend_from_slice(&self.ciphertext);
        bytes
    }

    /// Deserialize from bytes
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        if bytes.len() < 4 {
            return Err(CryptoError::DecryptionFailed(
                "Ciphertext too short".to_string(),
            ));
        }

        let encapped_len = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as usize;

        if bytes.len() < 4 + encapped_len {
            return Err(CryptoError::DecryptionFailed(
                "Invalid encapped key length".to_string(),
            ));
        }

        let encapped_key = bytes[4..4 + encapped_len].to_vec();
        let ciphertext = bytes[4 + encapped_len..].to_vec();

        Ok(HpkeCiphertext {
            encapped_key,
            ciphertext,
        })
    }
}

/// Encrypt data with HPKE (base mode - no associated data)
///
/// Used for sub-share encryption in resharing
///
/// Reference: 080 spec Part 4, Phase 1 (sub-share distribution)
pub fn encrypt_base<R: RngCore + CryptoRng>(
    plaintext: &[u8],
    recipient_pk: &HpkePublicKey,
    rng: &mut R,
) -> Result<HpkeCiphertext> {
    // Setup sender in base mode (no authentication, no associated data)
    let (encapped_key, mut encryption_context) = hpke::setup_sender::<AeadAlg, KdfAlg, KemAlg, _>(
        &OpModeS::Base,
        &recipient_pk.inner,
        b"aura.hpke.base.v1", // info string
        rng,
    )
    .map_err(|e| CryptoError::EncryptionFailed(format!("HPKE setup failed: {:?}", e)))?;

    // Encrypt the plaintext
    let ciphertext = encryption_context
        .seal(plaintext, b"") // no associated data in base mode
        .map_err(|e| CryptoError::EncryptionFailed(format!("HPKE encryption failed: {:?}", e)))?;

    Ok(HpkeCiphertext {
        encapped_key: encapped_key.to_bytes().to_vec(),
        ciphertext,
    })
}

/// Decrypt data with HPKE (base mode - no associated data)
///
/// Used for sub-share decryption in resharing
///
/// Reference: 080 spec Part 4, Phase 2 (share reconstruction)
pub fn decrypt_base(ciphertext: &HpkeCiphertext, recipient_sk: &HpkePrivateKey) -> Result<Vec<u8>> {
    // Deserialize encapped key
    let encapped_key = <KemAlg as Kem>::EncappedKey::from_bytes(&ciphertext.encapped_key)
        .map_err(|e| CryptoError::DecryptionFailed(format!("Invalid encapped key: {:?}", e)))?;

    // Setup receiver in base mode
    let mut decryption_context = hpke::setup_receiver::<AeadAlg, KdfAlg, KemAlg>(
        &OpModeR::Base,
        &recipient_sk.inner,
        &encapped_key,
        b"aura.hpke.base.v1", // info string (must match sender)
    )
    .map_err(|e| CryptoError::DecryptionFailed(format!("HPKE setup failed: {:?}", e)))?;

    // Decrypt the ciphertext
    let plaintext = decryption_context
        .open(&ciphertext.ciphertext, b"") // no associated data in base mode
        .map_err(|e| CryptoError::DecryptionFailed(format!("HPKE decryption failed: {:?}", e)))?;

    Ok(plaintext)
}

/// Encrypt data with HPKE with associated data (authenticated encryption)
///
/// Used for guardian share encryption with (request_id, guardian_id) as associated data
/// Provides replay protection by binding ciphertext to specific recovery session
///
/// Reference: 080 spec Part 2, Phase 1 (approval collection with replay protection)
pub fn encrypt_with_aad<R: RngCore + CryptoRng>(
    plaintext: &[u8],
    recipient_pk: &HpkePublicKey,
    associated_data: &[u8],
    rng: &mut R,
) -> Result<HpkeCiphertext> {
    // Setup sender in base mode
    let (encapped_key, mut encryption_context) = hpke::setup_sender::<AeadAlg, KdfAlg, KemAlg, _>(
        &OpModeS::Base,
        &recipient_pk.inner,
        b"aura.hpke.aad.v1", // info string
        rng,
    )
    .map_err(|e| CryptoError::EncryptionFailed(format!("HPKE setup failed: {:?}", e)))?;

    // Encrypt with associated data (AAD)
    let ciphertext = encryption_context
        .seal(plaintext, associated_data)
        .map_err(|e| CryptoError::EncryptionFailed(format!("HPKE encryption failed: {:?}", e)))?;

    Ok(HpkeCiphertext {
        encapped_key: encapped_key.to_bytes().to_vec(),
        ciphertext,
    })
}

/// Decrypt data with HPKE with associated data
///
/// AAD must match what was used during encryption, providing replay protection
///
/// Reference: 080 spec Part 2, Phase 3 (share reconstruction with proof verification)
pub fn decrypt_with_aad(
    ciphertext: &HpkeCiphertext,
    recipient_sk: &HpkePrivateKey,
    associated_data: &[u8],
) -> Result<Vec<u8>> {
    // Deserialize encapped key
    let encapped_key = <KemAlg as Kem>::EncappedKey::from_bytes(&ciphertext.encapped_key)
        .map_err(|e| CryptoError::DecryptionFailed(format!("Invalid encapped key: {:?}", e)))?;

    // Setup receiver
    let mut decryption_context = hpke::setup_receiver::<AeadAlg, KdfAlg, KemAlg>(
        &OpModeR::Base,
        &recipient_sk.inner,
        &encapped_key,
        b"aura.hpke.aad.v1", // info string (must match sender)
    )
    .map_err(|e| CryptoError::DecryptionFailed(format!("HPKE setup failed: {:?}", e)))?;

    // Decrypt with AAD validation
    let plaintext = decryption_context
        .open(&ciphertext.ciphertext, associated_data)
        .map_err(|e| {
            CryptoError::DecryptionFailed(format!(
                "HPKE decryption failed (AAD mismatch?): {:?}",
                e
            ))
        })?;

    Ok(plaintext)
}

#[cfg(test)]
#[allow(warnings, clippy::all)]
mod tests {
    use super::*;
    use crate::Effects;

    #[test]
    fn test_keypair_generation() {
        let effects = Effects::for_test("hpke_test");
        let mut rng = effects.rng();
        let keypair = HpkeKeyPair::generate(&mut rng);

        // Should be able to serialize and deserialize keys
        let pk_bytes = keypair.public_key.to_bytes();
        let sk_bytes = keypair.private_key.to_bytes();

        let pk_restored = HpkePublicKey::from_bytes(&pk_bytes).unwrap();
        let sk_restored = HpkePrivateKey::from_bytes(&sk_bytes).unwrap();

        assert_eq!(pk_restored.to_bytes(), pk_bytes);
        assert_eq!(sk_restored.to_bytes(), sk_bytes);
    }

    #[test]
    fn test_encrypt_decrypt_base() {
        let effects = Effects::for_test("hpke_test");
        let mut rng = effects.rng();
        let keypair = HpkeKeyPair::generate(&mut rng);

        let plaintext = b"secret sub-share data for resharing";

        // Encrypt
        let ciphertext = encrypt_base(plaintext, &keypair.public_key, &mut rng).unwrap();

        // Decrypt
        let decrypted = decrypt_base(&ciphertext, &keypair.private_key).unwrap();

        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn test_encrypt_decrypt_with_aad() {
        let effects = Effects::for_test("hpke_test");
        let mut rng = effects.rng();
        let keypair = HpkeKeyPair::generate(&mut rng);

        let plaintext = b"guardian share for recovery";
        let aad = b"request_id:12345678||guardian_id:alice";

        // Encrypt with AAD
        let ciphertext = encrypt_with_aad(plaintext, &keypair.public_key, aad, &mut rng).unwrap();

        // Decrypt with correct AAD
        let decrypted = decrypt_with_aad(&ciphertext, &keypair.private_key, aad).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn test_decrypt_with_wrong_aad_fails() {
        let effects = Effects::for_test("hpke_test");
        let mut rng = effects.rng();
        let keypair = HpkeKeyPair::generate(&mut rng);

        let plaintext = b"guardian share";
        let correct_aad = b"request_id:12345678||guardian_id:alice";
        let wrong_aad = b"request_id:87654321||guardian_id:bob";

        // Encrypt with correct AAD
        let ciphertext =
            encrypt_with_aad(plaintext, &keypair.public_key, correct_aad, &mut rng).unwrap();

        // Try to decrypt with wrong AAD - should fail
        let result = decrypt_with_aad(&ciphertext, &keypair.private_key, wrong_aad);
        assert!(result.is_err(), "Decryption with wrong AAD should fail");
    }

    #[test]
    fn test_decrypt_with_wrong_key_fails() {
        let effects = Effects::for_test("hpke_test");
        let mut rng = effects.rng();
        let keypair1 = HpkeKeyPair::generate(&mut rng);
        let keypair2 = HpkeKeyPair::generate(&mut rng);

        let plaintext = b"secret data";

        // Encrypt for keypair1
        let ciphertext = encrypt_base(plaintext, &keypair1.public_key, &mut rng).unwrap();

        // Try to decrypt with keypair2 - should fail
        let result = decrypt_base(&ciphertext, &keypair2.private_key);
        assert!(result.is_err(), "Decryption with wrong key should fail");
    }

    #[test]
    fn test_ciphertext_serialization() {
        let effects = Effects::for_test("hpke_test");
        let mut rng = effects.rng();
        let keypair = HpkeKeyPair::generate(&mut rng);

        let plaintext = b"test data";
        let ciphertext = encrypt_base(plaintext, &keypair.public_key, &mut rng).unwrap();

        // Serialize and deserialize
        let bytes = ciphertext.to_bytes();
        let restored = HpkeCiphertext::from_bytes(&bytes).unwrap();

        // Should be able to decrypt restored ciphertext
        let decrypted = decrypt_base(&restored, &keypair.private_key).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn test_guardian_share_encryption_with_replay_protection() {
        // Simulate guardian share encryption with replay protection
        let effects = Effects::for_test("hpke_test");
        let mut rng = effects.rng();

        // New device's keypair
        let new_device_keypair = HpkeKeyPair::generate(&mut rng);

        // Guardian's share
        let guardian_share = b"guardian_secret_share_12345";

        // Recovery context (request_id, guardian_id)
        let request_id = "recovery-req-abc123";
        let guardian_id = "guardian-alice";
        let aad = format!("{}||{}", request_id, guardian_id);

        // Guardian encrypts share for new device with AAD
        let ciphertext = encrypt_with_aad(
            guardian_share,
            &new_device_keypair.public_key,
            aad.as_bytes(),
            &mut rng,
        )
        .unwrap();

        // New device decrypts with correct AAD
        let decrypted =
            decrypt_with_aad(&ciphertext, &new_device_keypair.private_key, aad.as_bytes()).unwrap();

        assert_eq!(decrypted, guardian_share);

        // Attempt to replay in different recovery session - should fail
        let wrong_request_id = "recovery-req-xyz789";
        let wrong_aad = format!("{}||{}", wrong_request_id, guardian_id);

        let replay_result = decrypt_with_aad(
            &ciphertext,
            &new_device_keypair.private_key,
            wrong_aad.as_bytes(),
        );

        assert!(
            replay_result.is_err(),
            "Replay attack should be prevented by AAD mismatch"
        );
    }
}
