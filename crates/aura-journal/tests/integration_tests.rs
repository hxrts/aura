//! Integration tests for the Automerge-based journal

use aura_journal::{AccountState, Operation, SyncProtocol};
use aura_crypto::Effects;
use aura_types::{AccountIdExt, DeviceIdExt};

#[tokio::test]
async fn test_basic_state_operations() {
    let effects = Effects::test(42);
    let account_id = aura_types::AccountId::new_with_effects(&effects);
    let signing_key = aura_crypto::Ed25519SigningKey::from_bytes(&effects.random_bytes::<32>());
    let group_public_key = signing_key.verifying_key();
    
    let mut state = AccountState::new(account_id, group_public_key).unwrap();
    
    // Test epoch management
    assert_eq!(state.get_epoch(), 0);
    let changes = state.increment_epoch().unwrap();
    assert!(!changes.is_empty());
    assert_eq!(state.get_epoch(), 1);
}

#[tokio::test] 
async fn test_device_management() {
    let effects = Effects::test(42);
    let account_id = aura_types::AccountId::new_with_effects(&effects);
    let signing_key = aura_crypto::Ed25519SigningKey::from_bytes(&effects.random_bytes::<32>());
    let group_public_key = signing_key.verifying_key();
    let device_id = aura_types::DeviceId::new_with_effects(&effects);
    
    let mut state = AccountState::new(account_id, group_public_key).unwrap();
    
    let device = aura_journal::DeviceMetadata {
        device_id,
        device_name: "Test Device".to_string(),
        device_type: aura_journal::DeviceType::Native,
        public_key: group_public_key,
        added_at: 1000,
        last_seen: 1000,
        dkd_commitment_proofs: std::collections::BTreeMap::new(),
        next_nonce: 0,
        used_nonces: std::collections::BTreeSet::new(),
        key_share_epoch: 0,
    };
    
    // Add device
    let changes = state.add_device(device.clone()).unwrap();
    assert!(!changes.is_empty());
    assert_eq!(state.get_devices().len(), 1);
    assert!(state.has_device(&device_id));
    
    // Remove device
    let changes = state.remove_device(device_id).unwrap();
    assert!(!changes.is_empty());
    assert_eq!(state.get_devices().len(), 0);
    assert!(!state.has_device(&device_id));
}

#[tokio::test]
async fn test_sync_between_devices() {
    let effects = Effects::test(42);
    let account_id = aura_types::AccountId::new_with_effects(&effects);
    let signing_key = aura_crypto::Ed25519SigningKey::from_bytes(&effects.random_bytes::<32>());
    let group_public_key = signing_key.verifying_key();
    
    let device1 = aura_types::DeviceId::new_with_effects(&effects);
    let device2 = aura_types::DeviceId::new_with_effects(&effects);
    
    // Create two states
    let state1 = std::sync::Arc::new(tokio::sync::RwLock::new(
        AccountState::new(account_id, group_public_key).unwrap()
    ));
    let state2 = std::sync::Arc::new(tokio::sync::RwLock::new(
        AccountState::new(account_id, group_public_key).unwrap()
    ));
    
    // Create sync protocols
    let sync1 = SyncProtocol::new(device1, state1.clone());
    let sync2 = SyncProtocol::new(device2, state2.clone());
    
    // Make changes on device1
    {
        let mut s1 = state1.write().await;
        s1.increment_epoch().unwrap();
    }
    
    // Generate sync message from device1 to device2
    let msg = sync1.generate_sync_message(device2).await.unwrap();
    assert_eq!(msg.epoch, 1);
    
    // Apply sync message on device2
    let result = sync2.receive_sync_message(msg).await.unwrap();
    assert!(result.changes_applied > 0);
    assert_eq!(result.new_epoch, 1);
    
    // Verify states are synchronized
    {
        let s1 = state1.read().await;
        let s2 = state2.read().await;
        assert_eq!(s1.get_epoch(), s2.get_epoch());
    }
}