//! Test helper utilities for aura-protocol tests
//!
//! This module provides common test utilities to reduce duplication across
//! test modules and standardize test patterns.

use aura_crypto::Effects;
use aura_journal::{AccountState, DeviceMetadata, DeviceType};
use aura_types::{AccountId, DeviceId};
use ed25519_dalek::SigningKey;
use std::collections::{BTreeMap, BTreeSet};
use uuid::Uuid;

/// Create a standard test Effects instance
pub fn create_test_effects() -> Effects {
    Effects::test()
}

/// Create a test DeviceId
pub fn create_test_device_id() -> DeviceId {
    DeviceId::from(Uuid::from_u128(12345))
}

/// Create a test AccountId  
pub fn create_test_account_id() -> AccountId {
    AccountId(Uuid::from_u128(67890))
}

/// Create a basic AccountState for testing
pub fn create_test_account_state(device_id: DeviceId) -> AccountState {
    let account_id = create_test_account_id();
    let signing_key = SigningKey::from_bytes(&[1u8; 32]);
    let public_key = signing_key.verifying_key();
    
    let device_metadata = DeviceMetadata {
        device_id,
        device_name: "Test Device".to_string(),
        device_type: DeviceType::Native,
        public_key,
        added_at: 1000,
        last_seen: 1000,
        dkd_commitment_proofs: BTreeMap::new(),
        next_nonce: 1,
        used_nonces: BTreeSet::new(),
        key_share_epoch: 1,
    };
    
    AccountState::new(
        account_id,
        public_key,
        device_metadata,
        2, // threshold
        3, // total_participants
    )
}

/// Create a deterministic UUID for testing
pub fn create_test_uuid(seed: u128) -> Uuid {
    Uuid::from_u128(seed)
}