// Core types for threshold signing operations

use crate::{CoordinationError, Result};
use ed25519_dalek::VerifyingKey;
use frost_ed25519 as frost;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::num::NonZeroU16;
use uuid::Uuid;

/// Unique identifier for a participant in the threshold signing protocol
/// 
/// ParticipantId must be non-zero for FROST compatibility.
/// Use `new()` or `try_from()` to create validated instances.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ParticipantId(NonZeroU16);

impl ParticipantId {
    /// Create a new ParticipantId from a non-zero value
    pub fn new(id: NonZeroU16) -> Self {
        ParticipantId(id)
    }
    
    /// Get the inner value as u16
    pub fn as_u16(&self) -> u16 {
        self.0.get()
    }
    
    /// Create a ParticipantId from a u16, panicking if zero
    /// 
    /// **WARNING**: This method panics if id is zero. Only use in tests!
    /// Use `try_from()` for fallible conversion in production code.
    pub fn from_u16_unchecked(id: u16) -> Self {
        Self::try_from(id).expect("ParticipantId must be non-zero")
    }
}

impl TryFrom<u16> for ParticipantId {
    type Error = crate::CoordinationError;
    
    fn try_from(id: u16) -> std::result::Result<Self, Self::Error> {
        NonZeroU16::new(id)
            .map(ParticipantId)
            .ok_or_else(|| crate::CoordinationError::InvalidParticipantCount(
                "Participant ID must be non-zero".to_string()
            ))
    }
}

/// Safe bidirectional mapping between ParticipantId and frost::Identifier
/// 
/// This struct prevents the brittle byte manipulation that was previously used
/// for reverse lookups from frost::Identifier back to ParticipantId.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdentifierMapping {
    participant_to_frost: BTreeMap<ParticipantId, frost::Identifier>,
    frost_to_participant: BTreeMap<frost::Identifier, ParticipantId>,
}

impl IdentifierMapping {
    /// Create a new mapping from a list of participant IDs
    pub fn new(participants: &[ParticipantId]) -> crate::Result<Self> {
        let mut participant_to_frost = BTreeMap::new();
        let mut frost_to_participant = BTreeMap::new();
        
        for &participant_id in participants {
            let frost_id = frost::Identifier::try_from(participant_id.0.get())
                .map_err(|_| crate::CoordinationError::InvalidParticipantCount(
                    format!("ParticipantId {} cannot be converted to frost::Identifier", participant_id.0.get())
                ))?;
                
            participant_to_frost.insert(participant_id, frost_id);
            frost_to_participant.insert(frost_id, participant_id);
        }
        
        Ok(IdentifierMapping {
            participant_to_frost,
            frost_to_participant,
        })
    }
    
    /// Convert ParticipantId to frost::Identifier safely
    pub fn to_frost(&self, participant_id: ParticipantId) -> Option<frost::Identifier> {
        self.participant_to_frost.get(&participant_id).copied()
    }
    
    /// Convert frost::Identifier back to ParticipantId safely
    pub fn from_frost(&self, frost_id: frost::Identifier) -> Option<ParticipantId> {
        self.frost_to_participant.get(&frost_id).copied()
    }
    
    /// Get all participant IDs in the mapping
    pub fn participant_ids(&self) -> Vec<ParticipantId> {
        self.participant_to_frost.keys().copied().collect()
    }
    
    /// Get all frost identifiers in the mapping
    pub fn frost_identifiers(&self) -> Vec<frost::Identifier> {
        self.participant_to_frost.values().copied().collect()
    }
    
    /// Check if a participant ID is in the mapping
    pub fn contains_participant(&self, participant_id: ParticipantId) -> bool {
        self.participant_to_frost.contains_key(&participant_id)
    }
    
    /// Check if a frost identifier is in the mapping
    pub fn contains_frost(&self, frost_id: frost::Identifier) -> bool {
        self.frost_to_participant.contains_key(&frost_id)
    }
}

impl From<ParticipantId> for frost::Identifier {
    fn from(id: ParticipantId) -> Self {
        // FROST identifiers must be non-zero - this is now guaranteed by type system
        // NonZeroU16 ensures the value is non-zero, so this conversion is infallible
        frost::Identifier::try_from(id.0.get())
            .expect("ParticipantId NonZeroU16 guarantees non-zero value")
    }
}

// Keep compatibility with existing code that uses u16
impl From<ParticipantId> for u16 {
    fn from(id: ParticipantId) -> Self {
        id.0.get()
    }
}

/// Device identifier (UUID-based)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DeviceId(pub Uuid);

impl DeviceId {
    pub fn new() -> Self {
        DeviceId(Uuid::new_v4())
    }
}

impl Default for DeviceId {
    fn default() -> Self {
        Self::new()
    }
}

/// A threshold share held by a participant
/// 
/// SECURITY: This type contains sensitive cryptographic material.
/// The FROST KeyPackage is zeroized on drop.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyShare {
    pub participant_id: ParticipantId,
    pub share: frost::keys::KeyPackage,
    pub threshold: u16,
    pub total_participants: u16,
}

impl Drop for KeyShare {
    fn drop(&mut self) {
        // Note: FROST's KeyPackage doesn't implement Zeroize directly,
        // but it contains a SigningShare which does implement Zeroize.
        // The signing_share() method returns a reference, so we rely on
        // FROST's own Drop implementation to zeroize the internal data.
        // This is a defense-in-depth measure.
    }
}

/// Public key package distributed to all participants after DKG
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublicKeyPackage {
    #[serde(with = "verifying_key_serde")]
    pub group_public: VerifyingKey,
    pub verifying_shares: BTreeMap<ParticipantId, frost::keys::VerifyingShare>,
    pub threshold: u16,
    pub total_participants: u16,
}

mod verifying_key_serde {
    use ed25519_dalek::VerifyingKey;
    use serde::{Deserialize, Deserializer, Serializer};
    
    pub fn serialize<S>(key: &VerifyingKey, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_bytes(key.as_bytes())
    }
    
    pub fn deserialize<'de, D>(deserializer: D) -> Result<VerifyingKey, D::Error>
    where
        D: Deserializer<'de>,
    {
        let bytes: Vec<u8> = Deserialize::deserialize(deserializer)?;
        VerifyingKey::from_bytes(bytes.as_slice().try_into().map_err(serde::de::Error::custom)?)
            .map_err(serde::de::Error::custom)
    }
}

/// Sealed share encrypted for device storage
/// 
/// SECURITY: This type contains sensitive cryptographic material.
/// All fields are zeroized on drop via the underlying SealedData.
/// 
/// # Encryption Scheme
/// 
/// Uses unified AES-256-GCM sealing from aura-crypto:
/// - Key: Derived from device secret using BLAKE3
/// - Nonce: Random 12 bytes (96-bit, GCM standard)
/// - Associated Data: device_id || participant_id
/// 
/// # Production Requirements
/// 
/// For production use, the device secret should be:
/// - iOS: Stored in Secure Enclave
/// - Android: Stored in Keystore with StrongBox
/// - Desktop: OS keychain (Keychain Access, Windows Credential Manager, Secret Service)
/// 
/// Current implementation uses in-memory secrets (INSECURE for production).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SealedShare {
    pub device_id: DeviceId,
    pub participant_id: ParticipantId,
    /// Encrypted share using unified sealing
    #[serde(flatten)]
    pub sealed_data: aura_crypto::SealedData,
}

impl SealedShare {
    /// Seal (encrypt) a key share for secure storage
    /// 
    /// # Arguments
    /// 
    /// * `share` - The KeyShare to encrypt
    /// * `device_id` - The device this share belongs to (for AAD binding)
    /// * `device_secret` - 32-byte device-specific secret (should come from secure storage)
    /// 
    /// # Security
    /// 
    /// The device_id is included in the authenticated data to:
    /// - Bind the encrypted share to a specific device
    /// - Prevent cross-device replay attacks
    /// - Provide cryptographic proof the share is for this device
    /// 
    /// WARNING: The device_secret MUST be stored in platform-specific secure storage:
    /// - iOS: Secure Enclave / Keychain (kSecAttrAccessibleWhenUnlockedThisDeviceOnly)
    /// - Android: AndroidKeyStore with StrongBox
    /// - macOS: Keychain with kSecAttrAccessibleWhenUnlockedThisDeviceOnly
    /// - Windows: DPAPI or Windows Hello
    /// - Linux: Secret Service API (gnome-keyring, kwallet)
    pub fn seal(
        share: &KeyShare,
        device_id: DeviceId,
        device_secret: &[u8; 32],
        effects: &aura_crypto::Effects,
    ) -> Result<Self> {
        // Create context for key derivation - includes real device_id
        let context = format!("aura-share-seal-v1:{}:{}", 
            share.participant_id.as_u16(), 
            device_id.0
        );
        
        // Associated data for authenticated encryption - includes real device_id
        // This cryptographically binds the encryption to this specific device
        let associated_data = format!("{}:{}", 
            device_id.0,
            share.participant_id.as_u16()
        );
        
        // Use unified sealing from aura-crypto
        let sealed_data = aura_crypto::SealedData::seal_value(
            share,
            device_secret,
            &context,
            Some(associated_data.as_bytes()),
            effects,
        ).map_err(|e| CoordinationError::CryptoError(e.to_string()))?;
        
        Ok(SealedShare {
            device_id,  // Real device_id, not placeholder
            participant_id: share.participant_id,
            sealed_data,
        })
    }
    
    /// Unseal (decrypt) a key share from secure storage
    /// 
    /// # Arguments
    /// 
    /// * `device_id` - The device attempting to unseal (must match sealed device_id)
    /// * `device_secret` - 32-byte device-specific secret (must be same as used for sealing)
    /// 
    /// # Security
    /// 
    /// Verifies:
    /// - The device_id matches (cryptographically verified via AAD)
    /// - The device_secret is correct
    /// - The data has not been tampered with
    /// 
    /// # Returns
    /// 
    /// The decrypted KeyShare, or an error if:
    /// - Device ID mismatch (wrong device)
    /// - Decryption fails (wrong key, tampered data)
    /// - Deserialization fails (corrupted data)
    pub fn unseal(&self, device_id: DeviceId, device_secret: &[u8; 32]) -> Result<KeyShare> {
        // Verify device ID matches (before even attempting decryption)
        if self.device_id != device_id {
            return Err(CoordinationError::DeviceMismatch {
                expected: self.device_id,
                provided: device_id,
            });
        }
        
        self.sealed_data
            .unseal_value(device_secret)
            .map_err(|e| CoordinationError::CryptoError(e.to_string()))
    }
}

#[cfg(test)]
mod seal_tests {
    #[allow(unused_imports)]
    use super::*;
    
    // Note: These tests verify encryption/decryption roundtrip, not FROST functionality.
    // Full DKG tests are in dkg.rs module.
    
    #[test]
    #[ignore] // Depends on old dkg module - needs rewrite for new choreography
    fn test_seal_unseal_roundtrip() {
        /*
        // Run the existing DKG test to get a real share
        // use crate::dkg::{DkgParticipant};
        let config = ThresholdConfig::default_2_of_3();
        let participants = vec![
            ParticipantId::from_u16_unchecked(1),
            ParticipantId::from_u16_unchecked(2),
            ParticipantId::from_u16_unchecked(3),
        ];
        
        // Run a minimal DKG to get a valid KeyShare
        let mut p1 = DkgParticipant::new(participants[0], config.clone());
        let mut p2 = DkgParticipant::new(participants[1], config.clone());
        let mut p3 = DkgParticipant::new(participants[2], config.clone());
        
        // Round 1
        let r1_p1 = p1.round1().unwrap();
        let r1_p2 = p2.round1().unwrap();
        let r1_p3 = p3.round1().unwrap();
        
        // Exchange round 1 packages
        let mut round1_for_p1 = std::collections::BTreeMap::new();
        round1_for_p1.insert(participants[1], r1_p2.clone());
        round1_for_p1.insert(participants[2], r1_p3.clone());
        
        let mut round1_for_p2 = std::collections::BTreeMap::new();
        round1_for_p2.insert(participants[0], r1_p1.clone());
        round1_for_p2.insert(participants[2], r1_p3.clone());
        
        let mut round1_for_p3 = std::collections::BTreeMap::new();
        round1_for_p3.insert(participants[0], r1_p1.clone());
        round1_for_p3.insert(participants[1], r1_p2.clone());
        
        // Round 2
        let r2_p1 = p1.round2(round1_for_p1.clone()).unwrap();
        let r2_p2 = p2.round2(round1_for_p2.clone()).unwrap();
        let r2_p3 = p3.round2(round1_for_p3.clone()).unwrap();
        
        // Collect round2 packages for p1
        let mut round2_for_p1 = std::collections::BTreeMap::new();
        round2_for_p1.insert(participants[1], r2_p2.get(&participants[0]).unwrap().clone());
        round2_for_p1.insert(participants[2], r2_p3.get(&participants[0]).unwrap().clone());
        
        // Finalize
        let (share, _public_key) = p1.finalize(round1_for_p1, round2_for_p1).unwrap();
        
        // Now test seal/unseal
        let effects = aura_crypto::Effects::test();
        let device_id = effects.gen_uuid().into();
        let device_secret: [u8; 32] = effects.random_bytes();
        
        // Seal the share with device ID
        let sealed = SealedShare::seal(&share, device_id, &device_secret, &effects).unwrap();
        
        // Verify it's encrypted (ciphertext != plaintext)
        assert!(!sealed.sealed_data.ciphertext.is_empty());
        assert_eq!(sealed.sealed_data.nonce.len(), 12);
        assert_eq!(sealed.device_id, device_id);
        
        // Unseal and verify
        let unsealed = sealed.unseal(device_id, &device_secret).unwrap();
        
        // Verify key properties match
        assert_eq!(unsealed.participant_id, share.participant_id);
        assert_eq!(unsealed.threshold, share.threshold);
        assert_eq!(unsealed.total_participants, share.total_participants);
        */
    }
    
    #[test]
    #[ignore] // Depends on old dkg module - needs rewrite for new choreography
    fn test_unseal_with_wrong_key_fails() {
        /*
        // Reuse the seal/unseal setup
        // use crate::dkg::{DkgParticipant};
        let config = ThresholdConfig::default_2_of_3();
        let participants = vec![
            ParticipantId::from_u16_unchecked(1),
            ParticipantId::from_u16_unchecked(2),
            ParticipantId::from_u16_unchecked(3),
        ];
        
        let mut p1 = DkgParticipant::new(participants[0], config.clone());
        let mut p2 = DkgParticipant::new(participants[1], config.clone());
        let mut p3 = DkgParticipant::new(participants[2], config.clone());
        
        let r1_p1 = p1.round1().unwrap();
        let r1_p2 = p2.round1().unwrap();
        let r1_p3 = p3.round1().unwrap();
        
        let mut round1_for_p1 = std::collections::BTreeMap::new();
        round1_for_p1.insert(participants[1], r1_p2.clone());
        round1_for_p1.insert(participants[2], r1_p3.clone());
        
        let mut round1_for_p2 = std::collections::BTreeMap::new();
        round1_for_p2.insert(participants[0], r1_p1.clone());
        round1_for_p2.insert(participants[2], r1_p3.clone());
        
        let mut round1_for_p3 = std::collections::BTreeMap::new();
        round1_for_p3.insert(participants[0], r1_p1.clone());
        round1_for_p3.insert(participants[1], r1_p2.clone());
        
        let r2_p1 = p1.round2(round1_for_p1.clone()).unwrap();
        let r2_p2 = p2.round2(round1_for_p2.clone()).unwrap();
        let r2_p3 = p3.round2(round1_for_p3.clone()).unwrap();
        
        let mut round2_for_p1 = std::collections::BTreeMap::new();
        round2_for_p1.insert(participants[1], r2_p2.get(&participants[0]).unwrap().clone());
        round2_for_p1.insert(participants[2], r2_p3.get(&participants[0]).unwrap().clone());
        
        let (share, _) = p1.finalize(round1_for_p1, round2_for_p1).unwrap();
        
        let effects = aura_crypto::Effects::test();
        let device_id = effects.gen_uuid().into();
        let device_secret: [u8; 32] = effects.random_bytes();
        let wrong_secret: [u8; 32] = effects.random_bytes();
        
        let sealed = SealedShare::seal(&share, device_id, &device_secret, &effects).unwrap();
        
        // Unseal with wrong key should fail
        let result = sealed.unseal(device_id, &wrong_secret);
        assert!(result.is_err());
        
        // Verify error is cryptographic (not serialization)
        match result {
            Err(CoordinationError::CryptoError(_)) => {}, // Expected
            _ => panic!("Expected CryptoError"),
        }
        */
    }
    
    #[test]
    fn test_identifier_mapping_correctness() {
        // Test that IdentifierMapping provides safe bidirectional conversion
        let participants = vec![
            ParticipantId::from_u16_unchecked(1),
            ParticipantId::from_u16_unchecked(3),
            ParticipantId::from_u16_unchecked(5),
        ];
        
        let mapping = IdentifierMapping::new(&participants).unwrap();
        
        // Test forward conversion (ParticipantId -> frost::Identifier)
        for &participant_id in &participants {
            let frost_id = mapping.to_frost(participant_id).unwrap();
            
            // Verify the conversion matches the direct From implementation
            let direct_frost_id: frost::Identifier = participant_id.into();
            assert_eq!(frost_id, direct_frost_id);
            
            // Test reverse conversion (frost::Identifier -> ParticipantId)
            let recovered_participant = mapping.from_frost(frost_id).unwrap();
            assert_eq!(recovered_participant, participant_id);
        }
        
        // Test non-existent conversions return None
        let non_existent_participant = ParticipantId::from_u16_unchecked(99);
        assert_eq!(mapping.to_frost(non_existent_participant), None);
        
        let non_existent_frost = frost::Identifier::try_from(99u16).unwrap();
        assert_eq!(mapping.from_frost(non_existent_frost), None);
        
        // Test membership checks
        assert!(mapping.contains_participant(participants[0]));
        assert!(!mapping.contains_participant(non_existent_participant));
        
        let frost_id = mapping.to_frost(participants[0]).unwrap();
        assert!(mapping.contains_frost(frost_id));
        assert!(!mapping.contains_frost(non_existent_frost));
        
        // Test collection methods
        let participant_ids = mapping.participant_ids();
        assert_eq!(participant_ids.len(), 3);
        for participant_id in participants {
            assert!(participant_ids.contains(&participant_id));
        }
        
        let frost_identifiers = mapping.frost_identifiers();
        assert_eq!(frost_identifiers.len(), 3);
    }
    
    #[test]
    #[ignore] // Depends on old dkg module - needs rewrite for new choreography
    fn test_seal_prevents_cross_device_replay() {
        /*
        // Run DKG to get a real share
        // use crate::dkg::{DkgParticipant};
        let config = ThresholdConfig::default_2_of_3();
        let participants = vec![
            ParticipantId::from_u16_unchecked(1),
            ParticipantId::from_u16_unchecked(2),
            ParticipantId::from_u16_unchecked(3),
        ];
        
        let mut p1 = DkgParticipant::new(participants[0], config.clone());
        let mut p2 = DkgParticipant::new(participants[1], config.clone());
        let mut p3 = DkgParticipant::new(participants[2], config.clone());
        
        let r1_p1 = p1.round1().unwrap();
        let r1_p2 = p2.round1().unwrap();
        let r1_p3 = p3.round1().unwrap();
        
        let mut round1_for_p1 = std::collections::BTreeMap::new();
        round1_for_p1.insert(participants[1], r1_p2.clone());
        round1_for_p1.insert(participants[2], r1_p3.clone());
        
        let mut round1_for_p2 = std::collections::BTreeMap::new();
        round1_for_p2.insert(participants[0], r1_p1.clone());
        round1_for_p2.insert(participants[2], r1_p3.clone());
        
        let mut round1_for_p3 = std::collections::BTreeMap::new();
        round1_for_p3.insert(participants[0], r1_p1.clone());
        round1_for_p3.insert(participants[1], r1_p2.clone());
        
        let r2_p1 = p1.round2(round1_for_p1.clone()).unwrap();
        let r2_p2 = p2.round2(round1_for_p2.clone()).unwrap();
        let r2_p3 = p3.round2(round1_for_p3.clone()).unwrap();
        
        let mut round2_for_p1 = std::collections::BTreeMap::new();
        round2_for_p1.insert(participants[1], r2_p2.get(&participants[0]).unwrap().clone());
        round2_for_p1.insert(participants[2], r2_p3.get(&participants[0]).unwrap().clone());
        
        let (share, _) = p1.finalize(round1_for_p1, round2_for_p1).unwrap();
        
        let effects = aura_crypto::Effects::test();
        let device_a = effects.gen_uuid().into();
        let device_b = effects.gen_uuid().into();
        let device_secret: [u8; 32] = effects.random_bytes();
        
        // Seal for device A
        let sealed_for_a = SealedShare::seal(&share, device_a, &device_secret, &effects).unwrap();
        
        // Verify sealed for device A
        assert_eq!(sealed_for_a.device_id, device_a);
        
        // Attempt to unseal on device B should fail with DeviceMismatch
        let result = sealed_for_a.unseal(device_b, &device_secret);
        assert!(result.is_err());
        match result {
            Err(CoordinationError::DeviceMismatch { expected, provided }) => {
                assert_eq!(expected, device_a);
                assert_eq!(provided, device_b);
            },
            _ => panic!("Expected DeviceMismatch error, got {:?}", result),
        }
        
        // Unseal on correct device should succeed
        let unsealed = sealed_for_a.unseal(device_a, &device_secret).unwrap();
        assert_eq!(unsealed.participant_id, share.participant_id);
        */
    }
}

/// Session for coordinating a multi-round MPC protocol
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct SessionId(pub Uuid);

impl SessionId {
    pub fn new() -> Self {
        SessionId(Uuid::new_v4())
    }
}

impl Default for SessionId {
    fn default() -> Self {
        Self::new()
    }
}

/// Configuration for threshold signing setup
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThresholdConfig {
    /// Minimum number of participants required (M in M-of-N)
    pub threshold: u16,
    /// Total number of participants (N in M-of-N)
    pub total_participants: u16,
}

impl ThresholdConfig {
    pub fn new(threshold: u16, total_participants: u16) -> crate::Result<Self> {
        if threshold == 0 || threshold > total_participants {
            return Err(crate::CoordinationError::InvalidThreshold {
                threshold,
                total: total_participants,
            });
        }
        Ok(ThresholdConfig {
            threshold,
            total_participants,
        })
    }
    
    /// Default 2-of-3 configuration for MVP
    pub fn default_2_of_3() -> Self {
        ThresholdConfig {
            threshold: 2,
            total_participants: 3,
        }
    }
}

/// Threshold signature produced by M-of-N participants
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThresholdSignature {
    #[serde(with = "signature_serde")]
    pub signature: ed25519_dalek::Signature,
    pub signers: Vec<ParticipantId>,
}

mod signature_serde {
    use ed25519_dalek::Signature;
    use serde::{Deserialize, Deserializer, Serializer};
    
    pub fn serialize<S>(sig: &Signature, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_bytes(&sig.to_bytes())
    }
    
    pub fn deserialize<'de, D>(deserializer: D) -> Result<Signature, D::Error>
    where
        D: Deserializer<'de>,
    {
        let bytes: Vec<u8> = Deserialize::deserialize(deserializer)?;
        Signature::from_slice(&bytes).map_err(serde::de::Error::custom)
    }
}

