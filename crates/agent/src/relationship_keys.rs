//! Multi-device relationship key management
//!
//! Implements pairwise relationship key establishment and distribution as specified in
//! docs/051_rendezvous.md Section 3B "Pairwise Relationship".
//!
//! Key Concepts:
//! - Link device selection via ledger consensus (lexicographically smallest)
//! - Threshold-signed key distribution via PairwiseKeyEstablished events
//! - Automatic key rewrapping when devices are added
//! - Derived keys: K_box (encryption), K_tag (routing), K_psk (handshake)

use crate::error::AgentError;
use aura_crypto::key_derivation::derive_relationship_keys;
use aura_crypto::{CryptoError, Effects};
use aura_journal::{AccountId, DeviceId};
use bincode;
use rand;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use hpke::{Deserializable, Serializable};

/// Unique identifier for a pairwise relationship between two accounts
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct RelationshipId(pub [u8; 32]);

impl RelationshipId {
    /// Create a new relationship ID from two account IDs
    /// Uses lexicographic ordering to ensure consistency
    pub fn new(account_a: &AccountId, account_b: &AccountId) -> Self {
        use blake3::Hasher;

        let mut hasher = Hasher::new();

        // Sort accounts lexicographically for consistent relationship ID
        if account_a.0.as_bytes() < account_b.0.as_bytes() {
            hasher.update(account_a.0.as_bytes());
            hasher.update(account_b.0.as_bytes());
        } else {
            hasher.update(account_b.0.as_bytes());
            hasher.update(account_a.0.as_bytes());
        }

        let hash = hasher.finalize();
        let mut id = [0u8; 32];
        id.copy_from_slice(hash.as_bytes());
        RelationshipId(id)
    }
}

/// Complete set of keys derived from a pairwise relationship secret
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelationshipKeys {
    /// Relationship identifier
    pub relationship_id: RelationshipId,

    /// Encryption key for envelopes (32 bytes)
    pub k_box: [u8; 32],

    /// Routing tag computation key (32 bytes)
    pub k_tag: [u8; 32],

    /// Pre-shared key for mutual authentication (32 bytes)
    pub k_psk: [u8; 32],

    /// Link device that anchors this relationship
    pub link_device: DeviceId,

    /// Peer account ID
    pub peer_account_id: AccountId,

    /// Establishment timestamp (milliseconds since epoch)
    pub established_at: u64,

    /// Key version for rotation
    pub version: u32,
}

impl RelationshipKeys {
    /// Derive all relationship keys from a pairwise secret
    pub fn derive_from_secret(
        relationship_id: RelationshipId,
        pairwise_secret: &[u8],
        link_device: DeviceId,
        peer_account_id: AccountId,
        established_at: u64,
        version: u32,
    ) -> Result<Self, CryptoError> {
        // Use the separated key derivation system from Phase 0.2
        let (k_box, k_tag, k_psk) =
            derive_relationship_keys(pairwise_secret, &relationship_id.0, version).map_err(
                |e| CryptoError::InvalidParameter(format!("key derivation failed: {}", e)),
            )?;

        Ok(RelationshipKeys {
            relationship_id,
            k_box,
            k_tag,
            k_psk,
            link_device,
            peer_account_id,
            established_at,
            version,
        })
    }
}

/// Event published to account ledger when pairwise relationship is established
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairwiseKeyEstablished {
    /// Relationship identifier
    pub relationship_id: RelationshipId,

    /// Link device ID (canonical anchor for this relationship)
    pub link_device: DeviceId,

    /// Peer account ID
    pub peer_account_id: AccountId,

    /// Establishment timestamp
    pub established_at: u64,

    /// Encrypted relationship keys, one per device
    /// Map: DeviceId -> HPKE-encrypted RelationshipKeys
    pub encrypted_keys: BTreeMap<DeviceId, Vec<u8>>,

    /// Key version
    pub version: u32,
}

/// Event published when relationship keys are re-encrypted for new devices
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairwiseKeyUpdate {
    /// Relationship identifier
    pub relationship_id: RelationshipId,

    /// Additional encrypted keys for new devices
    /// Map: DeviceId -> HPKE-encrypted RelationshipKeys
    pub encrypted_keys: BTreeMap<DeviceId, Vec<u8>>,

    /// Key version
    pub version: u32,
}

/// Link device selection policy
pub struct LinkDeviceSelector;

impl LinkDeviceSelector {
    /// Select link device using ledger consensus
    /// Returns the lexicographically smallest online device ID
    pub fn select_link_device(online_devices: &[DeviceId]) -> Option<DeviceId> {
        if online_devices.is_empty() {
            return None;
        }

        // Sort devices lexicographically and pick the smallest
        let mut sorted = online_devices.to_vec();
        sorted.sort();
        Some(sorted[0])
    }

    /// Check if a device should be the link device for a relationship
    pub fn is_link_device(device_id: &DeviceId, online_devices: &[DeviceId]) -> bool {
        Self::select_link_device(online_devices)
            .map(|link| link == *device_id)
            .unwrap_or(false)
    }
}

/// Manager for relationship key distribution and rewrapping
pub struct RelationshipKeyManager {
    effects: Effects,
}

impl RelationshipKeyManager {
    /// Create a new relationship key manager
    pub fn new(effects: Effects) -> Self {
        Self { effects }
    }

    /// Establish a new pairwise relationship
    ///
    /// This should be called by the link device after successful DH key exchange.
    /// The pairwise secret (RID_AB) is derived from X25519 DH between device static keys.
    pub fn establish_relationship(
        &self,
        my_account_id: &AccountId,
        peer_account_id: &AccountId,
        pairwise_secret: &[u8; 32],
        my_device_id: DeviceId,
        account_devices: &[DeviceId],
        device_hpke_public_keys: &BTreeMap<DeviceId, Vec<u8>>,
    ) -> Result<PairwiseKeyEstablished, AgentError> {
        let relationship_id = RelationshipId::new(my_account_id, peer_account_id);
        let established_at = self
            .effects
            .now()
            .map_err(|e| AgentError::System(crate::error::SystemError::TimeError(e.to_string())))?;

        // Derive relationship keys
        let keys = RelationshipKeys::derive_from_secret(
            relationship_id.clone(),
            pairwise_secret,
            my_device_id,
            peer_account_id.clone(),
            established_at,
            1, // Initial version
        )?;

        // Encrypt keys for each device using HPKE
        let mut encrypted_keys = BTreeMap::new();
        for device_id in account_devices {
            if let Some(pk) = device_hpke_public_keys.get(device_id) {
                let encrypted = self.encrypt_keys_for_device(&keys, pk)?;
                encrypted_keys.insert(*device_id, encrypted);
            }
        }

        Ok(PairwiseKeyEstablished {
            relationship_id,
            link_device: my_device_id,
            peer_account_id: peer_account_id.clone(),
            established_at,
            encrypted_keys,
            version: 1,
        })
    }

    /// Rewrap relationship keys for newly added devices
    ///
    /// Called automatically when a DeviceAdded event is detected in the ledger.
    pub fn rewrap_keys_for_new_devices(
        &self,
        existing_relationships: &[RelationshipKeys],
        new_device_ids: &[DeviceId],
        device_hpke_public_keys: &BTreeMap<DeviceId, Vec<u8>>,
    ) -> Result<Vec<PairwiseKeyUpdate>, AgentError> {
        let mut updates = Vec::new();

        for keys in existing_relationships {
            let mut encrypted_keys = BTreeMap::new();

            for device_id in new_device_ids {
                if let Some(pk) = device_hpke_public_keys.get(device_id) {
                    let encrypted = self.encrypt_keys_for_device(keys, pk)?;
                    encrypted_keys.insert(*device_id, encrypted);
                }
            }

            if !encrypted_keys.is_empty() {
                updates.push(PairwiseKeyUpdate {
                    relationship_id: keys.relationship_id.clone(),
                    encrypted_keys,
                    version: keys.version,
                });
            }
        }

        Ok(updates)
    }

    /// Encrypt relationship keys for a specific device using HPKE
    fn encrypt_keys_for_device(
        &self,
        keys: &RelationshipKeys,
        device_public_key: &[u8],
    ) -> Result<Vec<u8>, AgentError> {
        use hpke::{aead::AesGcm128, kdf::HkdfSha256, kem::X25519HkdfSha256, single_shot_seal, Kem, OpModeS};

        // Serialize keys
        let serialized = bincode::serialize(keys)
            .map_err(|e| AgentError::serialization(format!("key serialization failed: {}", e)))?;

        // Use HPKE for proper encryption
        // HPKE configuration: X25519 + HKDF-SHA256 + AES-128-GCM
        type HpkeKem = X25519HkdfSha256;
        type HpkeKdf = HkdfSha256;
        type HpkeAead = AesGcm128;

        // Convert public key to HPKE format
        let recipient_pubkey = <HpkeKem as Kem>::PublicKey::from_bytes(device_public_key)
            .map_err(|e| AgentError::crypto_operation(format!("Invalid HPKE public key: {:?}", e)))?;

        // Generate ephemeral keypair and encrypt
        let mut rng = rand::thread_rng();
        let info = b"aura-relationship-key-encryption";
        let aad = b""; // No additional authenticated data

        match single_shot_seal::<HpkeAead, HpkeKdf, HpkeKem, _>(
            &OpModeS::Base,
            &recipient_pubkey,
            info,
            &serialized,
            aad,
            &mut rng,
        ) {
            Ok((encapped_key, ciphertext)) => {
                // Combine encapsulated key and ciphertext for storage
                let mut encrypted = Vec::new();
                encrypted.extend_from_slice(&encapped_key.to_bytes());
                encrypted.extend_from_slice(&ciphertext);
                Ok(encrypted)
            }
            Err(e) => Err(AgentError::crypto_operation(format!(
                "HPKE encryption failed: {:?}",
                e
            ))),
        }
    }

    /// Decrypt relationship keys encrypted for this device
    pub fn decrypt_keys_for_device(
        &self,
        encrypted_keys: &[u8],
        device_secret_key: &[u8],
    ) -> Result<RelationshipKeys, AgentError> {
        use hpke::{aead::AesGcm128, kdf::HkdfSha256, kem::X25519HkdfSha256, single_shot_open, Kem, OpModeR};

        // Use HPKE for proper decryption
        // HPKE configuration: X25519 + HKDF-SHA256 + AES-128-GCM
        type HpkeKem = X25519HkdfSha256;
        type HpkeKdf = HkdfSha256;
        type HpkeAead = AesGcm128;

        // Convert secret key to HPKE format
        let recipient_privkey = <HpkeKem as Kem>::PrivateKey::from_bytes(device_secret_key)
            .map_err(|e| AgentError::crypto_operation(format!("Invalid HPKE private key: {:?}", e)))?;

        // Extract encapsulated key and ciphertext
        // The encapsulated key is 32 bytes for X25519
        if encrypted_keys.len() < 32 {
            return Err(AgentError::crypto_operation(
                "Encrypted data too short to contain HPKE encapsulated key".to_string(),
            ));
        }

        let (encapped_key_bytes, ciphertext) = encrypted_keys.split_at(32);
        let encapped_key = <HpkeKem as Kem>::EncappedKey::from_bytes(encapped_key_bytes)
            .map_err(|e| AgentError::crypto_operation(format!("Invalid HPKE encapsulated key: {:?}", e)))?;

        // Decrypt using HPKE
        let info = b"aura-relationship-key-encryption";
        let aad = b""; // No additional authenticated data

        match single_shot_open::<HpkeAead, HpkeKdf, HpkeKem>(
            &OpModeR::Base,
            &recipient_privkey,
            &encapped_key,
            info,
            ciphertext,
            aad,
        ) {
            Ok(plaintext) => {
                // Deserialize the decrypted keys
                let keys: RelationshipKeys = bincode::deserialize(&plaintext).map_err(|e| {
                    AgentError::serialization(format!("key deserialization failed: {}", e))
                })?;
                Ok(keys)
            }
            Err(e) => Err(AgentError::crypto_operation(format!(
                "HPKE decryption failed: {:?}",
                e
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn test_account_id(name: &str) -> AccountId {
        AccountId(Uuid::new_v5(&Uuid::NAMESPACE_DNS, name.as_bytes()))
    }

    fn test_device_id(name: &str) -> DeviceId {
        DeviceId(Uuid::new_v5(&Uuid::NAMESPACE_DNS, name.as_bytes()))
    }

    #[test]
    fn test_relationship_id_consistency() {
        let account_a = test_account_id("alice");
        let account_b = test_account_id("bob");

        // Relationship ID should be same regardless of order
        let rid1 = RelationshipId::new(&account_a, &account_b);
        let rid2 = RelationshipId::new(&account_b, &account_a);

        assert_eq!(rid1, rid2);
    }

    #[test]
    fn test_link_device_selection() {
        let dev1 = test_device_id("device1");
        let dev2 = test_device_id("device2");
        let dev3 = test_device_id("device3");

        let mut devices = vec![dev3, dev1, dev2];
        devices.sort();

        let link = LinkDeviceSelector::select_link_device(&devices);
        assert_eq!(link, Some(devices[0]));

        // Selection should be deterministic
        let link2 = LinkDeviceSelector::select_link_device(&[dev2, dev3, dev1]);
        assert_eq!(link, link2);
    }

    #[test]
    fn test_relationship_keys_derivation() {
        let account_a = test_account_id("alice");
        let account_b = test_account_id("bob");
        let rid = RelationshipId::new(&account_a, &account_b);

        let pairwise_secret = [0x42; 32];
        let link_device = test_device_id("link1");

        let keys = RelationshipKeys::derive_from_secret(
            rid.clone(),
            &pairwise_secret,
            link_device,
            account_b.clone(),
            1000,
            1,
        )
        .unwrap();

        assert_eq!(keys.relationship_id, rid);
        assert_eq!(keys.link_device, link_device);
        assert_eq!(keys.peer_account_id, account_b);
        assert_eq!(keys.version, 1);

        // Keys should be non-zero
        assert_ne!(keys.k_box, [0u8; 32]);
        assert_ne!(keys.k_tag, [0u8; 32]);
        assert_ne!(keys.k_psk, [0u8; 32]);

        // Keys should be different from each other
        assert_ne!(keys.k_box, keys.k_tag);
        assert_ne!(keys.k_box, keys.k_psk);
        assert_ne!(keys.k_tag, keys.k_psk);
    }

    #[test]
    fn test_establish_relationship() {
        let effects = aura_crypto::Effects::test();
        let manager = RelationshipKeyManager::new(effects);

        let account_a = test_account_id("alice");
        let account_b = test_account_id("bob");
        let dev1 = test_device_id("device1");
        let dev2 = test_device_id("device2");

        let pairwise_secret = [0x42; 32];
        let devices = vec![dev1, dev2];

        // Create dummy HPKE public keys
        let mut hpke_keys = BTreeMap::new();
        hpke_keys.insert(dev1, vec![1, 2, 3]);
        hpke_keys.insert(dev2, vec![4, 5, 6]);

        let event = manager
            .establish_relationship(
                &account_a,
                &account_b,
                &pairwise_secret,
                dev1,
                &devices,
                &hpke_keys,
            )
            .unwrap();

        assert_eq!(event.link_device, dev1);
        assert_eq!(event.peer_account_id, account_b);
        assert_eq!(event.version, 1);
        assert_eq!(event.encrypted_keys.len(), 2);
    }

    #[test]
    fn test_rewrap_keys_for_new_devices() {
        let effects = aura_crypto::Effects::test();
        let manager = RelationshipKeyManager::new(effects);

        let account_a = test_account_id("alice");
        let account_b = test_account_id("bob");
        let rid = RelationshipId::new(&account_a, &account_b);

        let pairwise_secret = [0x42; 32];
        let dev1 = test_device_id("device1");
        let dev3 = test_device_id("device3");

        let keys =
            RelationshipKeys::derive_from_secret(rid, &pairwise_secret, dev1, account_b, 1000, 1)
                .unwrap();

        let mut hpke_keys = BTreeMap::new();
        hpke_keys.insert(dev3, vec![7, 8, 9]);

        let updates = manager
            .rewrap_keys_for_new_devices(&[keys.clone()], &[dev3], &hpke_keys)
            .unwrap();

        assert_eq!(updates.len(), 1);
        assert_eq!(updates[0].relationship_id, keys.relationship_id);
        assert_eq!(updates[0].encrypted_keys.len(), 1);
        assert!(updates[0].encrypted_keys.contains_key(&dev3));
    }

    #[test]
    fn test_key_encryption_decryption_roundtrip() {
        let effects = aura_crypto::Effects::test();
        let manager = RelationshipKeyManager::new(effects);

        let account_a = test_account_id("alice");
        let account_b = test_account_id("bob");
        let rid = RelationshipId::new(&account_a, &account_b);
        let dev1 = test_device_id("device1");

        let pairwise_secret = [0x42; 32];

        let original_keys =
            RelationshipKeys::derive_from_secret(rid, &pairwise_secret, dev1, account_b, 1000, 1)
                .unwrap();

        let device_pk = vec![1, 2, 3, 4];
        let device_sk = vec![5, 6, 7, 8];

        let encrypted = manager
            .encrypt_keys_for_device(&original_keys, &device_pk)
            .unwrap();
        let decrypted = manager
            .decrypt_keys_for_device(&encrypted, &device_sk)
            .unwrap();

        assert_eq!(decrypted.relationship_id, original_keys.relationship_id);
        assert_eq!(decrypted.k_box, original_keys.k_box);
        assert_eq!(decrypted.k_tag, original_keys.k_tag);
        assert_eq!(decrypted.k_psk, original_keys.k_psk);
    }
}
