//! Test Device Utilities
//!
//! Factory functions for creating test DeviceMetadata instances.
//! Consolidates device creation patterns found across multiple test files.

use aura_crypto::Effects;
use aura_journal::{DeviceId, DeviceMetadata, DeviceType, current_timestamp_with_effects};
use ed25519_dalek::{SigningKey, VerifyingKey};
use std::collections::BTreeMap;
use uuid::Uuid;

/// Create a test device with given effects
/// 
/// Standard pattern for creating test devices with deterministic properties.
/// 
/// # Arguments
/// * `effects` - Effects instance for deterministic generation
pub fn test_device_with_effects(effects: &Effects) -> DeviceMetadata {
    let key_bytes = effects.random_bytes::<32>();
    let signing_key = SigningKey::from_bytes(&key_bytes);
    let public_key = signing_key.verifying_key();
    
    let device_id = DeviceId::new_with_effects(effects);
    let timestamp = current_timestamp_with_effects(effects)
        .unwrap(); // Timestamp generation should succeed in tests
    
    DeviceMetadata {
        device_id,
        device_name: "Test Device".to_string(),
        device_type: DeviceType::Native,
        public_key,
        added_at: timestamp,
        last_seen: timestamp,
        dkd_commitment_proofs: BTreeMap::new(),
        next_nonce: 0,
        used_nonces: std::collections::BTreeSet::new(),
    }
}

/// Create a test device with specific numeric ID
/// 
/// Useful for creating devices with predictable IDs for testing.
/// This matches the `mock_device(id: u16, effects)` pattern found in multiple files.
/// 
/// # Arguments
/// * `id` - Numeric ID to convert to UUID
/// * `effects` - Effects instance for other random generation
pub fn test_device_with_id(id: u16, effects: &Effects) -> DeviceMetadata {
    let key_bytes = effects.random_bytes::<32>();
    let signing_key = SigningKey::from_bytes(&key_bytes);
    let public_key = signing_key.verifying_key();
    
    let device_id = DeviceId(Uuid::from_u128(id as u128));
    let timestamp = current_timestamp_with_effects(effects)
        .unwrap(); // Timestamp generation should succeed in tests
    
    DeviceMetadata {
        device_id,
        device_name: format!("Device {}", id),
        device_type: DeviceType::Native,
        public_key,
        added_at: timestamp,
        last_seen: timestamp,
        dkd_commitment_proofs: BTreeMap::new(),
        next_nonce: 0,
        used_nonces: std::collections::BTreeSet::new(),
    }
}

/// Create a test device with specific name
/// 
/// For tests that need named devices for clarity.
/// 
/// # Arguments
/// * `name` - Device name
/// * `effects` - Effects instance for random generation
pub fn test_device_with_name(name: &str, effects: &Effects) -> DeviceMetadata {
    let key_bytes = effects.random_bytes::<32>();
    let signing_key = SigningKey::from_bytes(&key_bytes);
    let public_key = signing_key.verifying_key();
    
    let device_id = DeviceId::new_with_effects(effects);
    let timestamp = current_timestamp_with_effects(effects)
        .unwrap(); // Timestamp generation should succeed in tests
    
    DeviceMetadata {
        device_id,
        device_name: name.to_string(),
        device_type: DeviceType::Native,
        public_key,
        added_at: timestamp,
        last_seen: timestamp,
        dkd_commitment_proofs: BTreeMap::new(),
        next_nonce: 0,
        used_nonces: std::collections::BTreeSet::new(),
    }
}

/// Create a test device with specific public key
/// 
/// For tests that need to control the device's public key.
/// 
/// # Arguments
/// * `public_key` - Specific public key to use
/// * `effects` - Effects instance for other random generation
pub fn test_device_with_key(public_key: VerifyingKey, effects: &Effects) -> DeviceMetadata {
    let device_id = DeviceId::new_with_effects(effects);
    let timestamp = current_timestamp_with_effects(effects)
        .unwrap(); // Timestamp generation should succeed in tests
    
    DeviceMetadata {
        device_id,
        device_name: "Test Device".to_string(),
        device_type: DeviceType::Native,
        public_key,
        added_at: timestamp,
        last_seen: timestamp,
        dkd_commitment_proofs: BTreeMap::new(),
        next_nonce: 0,
        used_nonces: std::collections::BTreeSet::new(),
    }
}

/// Create a test device with specific type
/// 
/// For testing different device types.
/// 
/// # Arguments
/// * `device_type` - Type of device (Native, Browser, etc.)
/// * `effects` - Effects instance for random generation
pub fn test_device_with_type(device_type: DeviceType, effects: &Effects) -> DeviceMetadata {
    let key_bytes = effects.random_bytes::<32>();
    let signing_key = SigningKey::from_bytes(&key_bytes);
    let public_key = signing_key.verifying_key();
    
    let device_id = DeviceId::new_with_effects(effects);
    let timestamp = current_timestamp_with_effects(effects)
        .unwrap(); // Timestamp generation should succeed in tests
    
    DeviceMetadata {
        device_id,
        device_name: format!("{:?} Device", device_type),
        device_type,
        public_key,
        added_at: timestamp,
        last_seen: timestamp,
        dkd_commitment_proofs: BTreeMap::new(),
        next_nonce: 0,
        used_nonces: std::collections::BTreeSet::new(),
    }
}

/// Create multiple test devices with sequential IDs
/// 
/// Useful for tests that need multiple devices.
/// 
/// # Arguments
/// * `count` - Number of devices to create
/// * `effects` - Effects instance for random generation
pub fn test_devices_sequential(count: u16, effects: &Effects) -> Vec<DeviceMetadata> {
    (1..=count).map(|id| test_device_with_id(id, effects)).collect()
}