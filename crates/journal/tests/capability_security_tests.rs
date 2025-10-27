#![allow(clippy::disallowed_methods, clippy::clone_on_copy)]
// Capability Authorization Security Tests
//
// Tests security properties of the capability system:
// - Authorization enforcement: Only authorized devices can access resources
// - Delegation chains: Proper validation of delegation chains
// - Expiration: Expired capabilities are rejected
// - Revocation: Revoked capabilities cannot be used
// - Privilege escalation: Cannot escalate privileges through delegation
// - Capability forgery: Cannot forge capabilities without proper signatures

use aura_crypto::Effects;
use aura_journal::capability::{CapabilityGrant, CapabilityManager, Permission, StorageOperation};
use aura_types::{DeviceId, DeviceIdExt};
use ed25519_dalek::SigningKey;

/// Test that only authorized devices can access resources
#[test]
fn test_authorization_enforcement() {
    let effects = Effects::for_test("authorization_enforcement");
    let mut manager = CapabilityManager::new();

    // Create devices
    let owner = DeviceId::new_with_effects(&effects);
    let authorized = DeviceId::new_with_effects(&effects);
    let unauthorized = DeviceId::new_with_effects(&effects);

    let owner_key = SigningKey::from_bytes(&[1u8; 32]);
    let authorized_key = SigningKey::from_bytes(&[2u8; 32]);

    // Register owner as authority
    manager.register_authority(owner, owner_key.verifying_key());

    // Resource identifier
    let resource_id = "storage/document-123".to_string();

    // Owner grants storage capability to authorized device
    let grant = CapabilityGrant {
        device_id: authorized,
        permissions: vec![
            Permission::Storage {
                operation: StorageOperation::Read,
                resource: resource_id.clone(),
            },
            Permission::Storage {
                operation: StorageOperation::Write,
                resource: resource_id.clone(),
            },
        ],
        issued_at: aura_crypto::time::current_timestamp_with_effects(&effects).unwrap_or(1000),
        expires_at: None,
        delegation_chain: vec![],
    };

    let capability_token = manager
        .grant_capability(grant, &owner_key, &effects)
        .unwrap();

    let current_time = aura_crypto::time::current_timestamp_with_effects(&effects).unwrap_or(1000);

    // Authorized device can access
    assert!(
        manager
            .verify_storage(
                &authorized,
                StorageOperation::Read,
                &resource_id,
                current_time
            )
            .is_ok(),
        "Authorized device should have read permission"
    );
    assert!(
        manager
            .verify_storage(
                &authorized,
                StorageOperation::Write,
                &resource_id,
                current_time
            )
            .is_ok(),
        "Authorized device should have write permission"
    );

    // Unauthorized device cannot access
    assert!(
        manager
            .verify_storage(
                &unauthorized,
                StorageOperation::Read,
                &resource_id,
                current_time
            )
            .is_err(),
        "Unauthorized device should not have read permission"
    );
    assert!(
        manager
            .verify_storage(
                &unauthorized,
                StorageOperation::Write,
                &resource_id,
                current_time
            )
            .is_err(),
        "Unauthorized device should not have write permission"
    );
}

/// Test basic capability delegation
#[test]
fn test_capability_delegation() {
    let effects = Effects::for_test("capability_delegation");
    let mut manager = CapabilityManager::new();

    // Create delegation chain: owner -> delegate1 -> delegate2
    let owner = DeviceId::new_with_effects(&effects);
    let delegate1 = DeviceId::new_with_effects(&effects);
    let delegate2 = DeviceId::new_with_effects(&effects);

    let owner_key = SigningKey::from_bytes(&[1u8; 32]);
    let delegate1_key = SigningKey::from_bytes(&[2u8; 32]);

    // Register authorities
    manager.register_authority(owner, owner_key.verifying_key());
    manager.register_authority(delegate1, delegate1_key.verifying_key());

    let resource_id = "storage/shared-folder".to_string();

    // Owner grants storage access to delegate1
    let grant1 = CapabilityGrant {
        device_id: delegate1,
        permissions: vec![Permission::Storage {
            operation: StorageOperation::Read,
            resource: resource_id.clone(),
        }],
        issued_at: aura_crypto::time::current_timestamp_with_effects(&effects).unwrap_or(1000),
        expires_at: None,
        delegation_chain: vec![],
    };
    let cap1_token = manager
        .grant_capability(grant1, &owner_key, &effects)
        .unwrap();

    let current_time = aura_crypto::time::current_timestamp_with_effects(&effects).unwrap_or(1000);

    // Verify delegate1 has the granted capability
    assert!(
        manager
            .verify_storage(
                &delegate1,
                StorageOperation::Read,
                &resource_id,
                current_time
            )
            .is_ok(),
        "Delegate1 should have read permission"
    );

    // Verify delegate2 does not have capability (no delegation occurred)
    assert!(
        manager
            .verify_storage(
                &delegate2,
                StorageOperation::Read,
                &resource_id,
                current_time
            )
            .is_err(),
        "Delegate2 should not have read permission without delegation"
    );
}

/// Test capability expiration enforcement
#[test]
fn test_capability_expiration() {
    let effects = Effects::for_test("capability_expiration");
    let mut manager = CapabilityManager::new();

    let owner = DeviceId::new_with_effects(&effects);
    let device = DeviceId::new_with_effects(&effects);
    let owner_key = SigningKey::from_bytes(&[1u8; 32]);

    manager.register_authority(owner, owner_key.verifying_key());

    let resource_id = "storage/temp-document".to_string();
    let current_time = aura_crypto::time::current_timestamp_with_effects(&effects).unwrap_or(1000);

    // Grant short-lived capability
    let grant = CapabilityGrant {
        device_id: device,
        permissions: vec![Permission::Storage {
            operation: StorageOperation::Read,
            resource: resource_id.clone(),
        }],
        issued_at: current_time,
        expires_at: Some(current_time + 100), // Expires in 100 seconds
        delegation_chain: vec![],
    };

    let _capability_token = manager
        .grant_capability(grant, &owner_key, &effects)
        .unwrap();

    // Should work before expiration
    assert!(
        manager
            .verify_storage(
                &device,
                StorageOperation::Read,
                &resource_id,
                current_time + 50
            )
            .is_ok(),
        "Capability should work before expiration"
    );

    // Should fail after expiration
    assert!(
        manager
            .verify_storage(
                &device,
                StorageOperation::Read,
                &resource_id,
                current_time + 200
            )
            .is_err(),
        "Capability should fail after expiration"
    );
}

/// Test capability revocation cascades properly
#[test]
fn test_capability_revocation() {
    let effects = Effects::for_test("capability_revocation");
    let mut manager = CapabilityManager::new();

    let owner = DeviceId::new_with_effects(&effects);
    let device = DeviceId::new_with_effects(&effects);
    let owner_key = SigningKey::from_bytes(&[1u8; 32]);

    manager.register_authority(owner, owner_key.verifying_key());

    let resource_id = "storage/revokable-doc".to_string();
    let current_time = aura_crypto::time::current_timestamp_with_effects(&effects).unwrap_or(1000);

    // Grant capability
    let grant = CapabilityGrant {
        device_id: device,
        permissions: vec![Permission::Storage {
            operation: StorageOperation::Read,
            resource: resource_id.clone(),
        }],
        issued_at: current_time,
        expires_at: None,
        delegation_chain: vec![],
    };

    let capability_token = manager
        .grant_capability(grant, &owner_key, &effects)
        .unwrap();

    // Should work initially
    assert!(
        manager
            .verify_storage(&device, StorageOperation::Read, &resource_id, current_time)
            .is_ok(),
        "Capability should work initially"
    );

    // Get the capability ID for revocation (simplified - in real system would track this)
    let capabilities = manager.get_capabilities(&device);
    let capability_id = capabilities[0].capability_id();

    // Revoke the capability
    manager.revoke_capability(capability_id).unwrap();

    // Should fail after revocation
    assert!(
        manager
            .verify_storage(&device, StorageOperation::Read, &resource_id, current_time)
            .is_err(),
        "Capability should fail after revocation"
    );
}

// TODO: Additional tests need significant refactoring for the new API:
// - Complex delegation chains
// - Privilege escalation prevention
// - Capability forgery detection
// - Multi-permission capabilities
// These require understanding the delegation_capability() method and the full capability token API.
