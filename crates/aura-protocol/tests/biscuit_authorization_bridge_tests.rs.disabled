//! Tests for Biscuit Authorization Bridge
//!
//! Tests the new BiscuitAuthorizationBridge implementation that provides
//! cryptographic token verification and Datalog policy evaluation.

use aura_core::{AuthorityId, ContextId, identifiers::DeviceId};
use aura_protocol::authorization::BiscuitAuthorizationBridge;
use aura_wot::{AccountAuthority, AuthorityOp, BiscuitTokenManager, ContextOp, ResourceScope};
use biscuit_auth::PublicKey;

/// Test successful authorization with valid Biscuit token
#[tokio::test]
async fn test_biscuit_bridge_successful_authorization() {
    // Create test authority
    let authority = AccountAuthority::new(aura_core::AccountId::new());
    let device_id = DeviceId::new();
    
    // Create device token
    let device_token = authority.create_device_token(device_id).unwrap();
    
    // Create authorization bridge
    let bridge = BiscuitAuthorizationBridge::new(authority.root_public_key(), device_id);
    
    // Create resource scope for authority operation
    let authority_id = AuthorityId::new();
    let resource_scope = ResourceScope::Authority {
        authority_id,
        operation: AuthorityOp::AddDevice,
    };
    
    // Test authorization - should succeed for "write" operation (device token has write capability)
    let result = bridge.authorize(&device_token, "write", &resource_scope).unwrap();
    
    assert!(result.authorized, "Valid token should be authorized");
    assert!(result.delegation_depth.is_some(), "Should extract delegation depth");
    assert!(!result.token_facts.is_empty(), "Should extract token facts");
}

/// Test authorization failure with operation not permitted by token
#[tokio::test]
async fn test_biscuit_bridge_unauthorized_operation() {
    // Create test authority
    let authority = AccountAuthority::new(aura_core::AccountId::new());
    let device_id = DeviceId::new();
    
    // Create device token
    let device_token = authority.create_device_token(device_id).unwrap();
    
    // Attenuate token to read-only
    let token_manager = BiscuitTokenManager::new(device_id, device_token);
    let read_only_token = token_manager.attenuate_read("storage/*").unwrap();
    
    // Create authorization bridge
    let bridge = BiscuitAuthorizationBridge::new(authority.root_public_key(), device_id);
    
    // Create resource scope for write operation
    let authority_id = AuthorityId::new();
    let resource_scope = ResourceScope::Storage {
        authority_id,
        path: "test/file.txt".to_string(),
    };
    
    // Test authorization for write operation with read-only token - should fail
    let result = bridge.authorize(&read_only_token, "write", &resource_scope).unwrap();
    
    assert!(!result.authorized, "Read-only token should not authorize write operations");
}

/// Test capability checking functionality
#[tokio::test]
async fn test_biscuit_bridge_capability_checking() {
    // Create test authority
    let authority = AccountAuthority::new(aura_core::AccountId::new());
    let device_id = DeviceId::new();
    
    // Create device token (has read, write, execute, delegate, admin capabilities)
    let device_token = authority.create_device_token(device_id).unwrap();
    
    // Create authorization bridge
    let bridge = BiscuitAuthorizationBridge::new(authority.root_public_key(), device_id);
    
    // Test various capabilities
    assert!(bridge.has_capability(&device_token, "read").unwrap(), "Device token should have read capability");
    assert!(bridge.has_capability(&device_token, "write").unwrap(), "Device token should have write capability");
    assert!(bridge.has_capability(&device_token, "execute").unwrap(), "Device token should have execute capability");
    assert!(bridge.has_capability(&device_token, "admin").unwrap(), "Device token should have admin capability");
    
    // Test non-existent capability
    assert!(!bridge.has_capability(&device_token, "nonexistent").unwrap(), "Device token should not have non-existent capability");
}

/// Test delegation depth extraction
#[tokio::test]
async fn test_biscuit_bridge_delegation_depth() {
    // Create test authority
    let authority = AccountAuthority::new(aura_core::AccountId::new());
    let device_id = DeviceId::new();
    
    // Create device token (delegation depth 0 - authority block only)
    let device_token = authority.create_device_token(device_id).unwrap();
    
    // Create authorization bridge
    let bridge = BiscuitAuthorizationBridge::new(authority.root_public_key(), device_id);
    
    // Create resource scope
    let authority_id = AuthorityId::new();
    let resource_scope = ResourceScope::Authority {
        authority_id,
        operation: AuthorityOp::AddDevice,
    };
    
    // Test delegation depth for base token
    let result = bridge.authorize(&device_token, "read", &resource_scope).unwrap();
    assert_eq!(result.delegation_depth, Some(0), "Base token should have delegation depth 0");
    
    // Create attenuated token (delegation depth 1)
    let token_manager = BiscuitTokenManager::new(device_id, device_token);
    let attenuated_token = token_manager.attenuate_read("storage/*").unwrap();
    
    let result2 = bridge.authorize(&attenuated_token, "read", &resource_scope).unwrap();
    assert_eq!(result2.delegation_depth, Some(1), "Attenuated token should have delegation depth 1");
}

/// Test different resource scopes
#[tokio::test]
async fn test_biscuit_bridge_resource_scopes() {
    // Create test authority
    let authority = AccountAuthority::new(aura_core::AccountId::new());
    let device_id = DeviceId::new();
    
    // Create device token
    let device_token = authority.create_device_token(device_id).unwrap();
    
    // Create authorization bridge
    let bridge = BiscuitAuthorizationBridge::new(authority.root_public_key(), device_id);
    
    // Test Authority resource scope
    let authority_resource = ResourceScope::Authority {
        authority_id: AuthorityId::new(),
        operation: AuthorityOp::AddGuardian,
    };
    let auth_result = bridge.authorize(&device_token, "admin", &authority_resource).unwrap();
    assert!(auth_result.authorized, "Admin operation should be authorized");
    
    // Test Context resource scope
    let context_resource = ResourceScope::Context {
        context_id: ContextId::new(),
        operation: ContextOp::ApproveRecovery,
    };
    let ctx_result = bridge.authorize(&device_token, "execute", &context_resource).unwrap();
    assert!(ctx_result.authorized, "Execute operation should be authorized");
    
    // Test Storage resource scope
    let storage_resource = ResourceScope::Storage {
        authority_id: AuthorityId::new(),
        path: "documents/test.txt".to_string(),
    };
    let storage_result = bridge.authorize(&device_token, "read", &storage_resource).unwrap();
    assert!(storage_result.authorized, "Read operation should be authorized");
}

/// Test token facts extraction
#[tokio::test]
async fn test_biscuit_bridge_token_facts_extraction() {
    // Create test authority
    let authority = AccountAuthority::new(aura_core::AccountId::new());
    let device_id = DeviceId::new();
    
    // Create device token
    let device_token = authority.create_device_token(device_id).unwrap();
    
    // Create authorization bridge
    let bridge = BiscuitAuthorizationBridge::new(authority.root_public_key(), device_id);
    
    // Create resource scope
    let authority_id = AuthorityId::new();
    let resource_scope = ResourceScope::Authority {
        authority_id,
        operation: AuthorityOp::UpdateTree,
    };
    
    // Test token facts extraction
    let result = bridge.authorize(&device_token, "write", &resource_scope).unwrap();
    
    // Verify that facts are extracted
    assert!(!result.token_facts.is_empty(), "Should extract token facts");
    
    // Look for device ID in facts
    let device_fact_found = result.token_facts.iter()
        .any(|fact| fact.contains(&format!("device(\"{}\")", device_id)));
    assert!(device_fact_found, "Should contain device fact");
    
    // Look for verification timestamp
    let timestamp_fact_found = result.token_facts.iter()
        .any(|fact| fact.contains("verified_at("));
    assert!(timestamp_fact_found, "Should contain verification timestamp");
}

/// Test authorization with wrong public key (should fail)
#[tokio::test]
async fn test_biscuit_bridge_wrong_public_key() {
    // Create test authority
    let authority = AccountAuthority::new(aura_core::AccountId::new());
    let device_id = DeviceId::new();
    
    // Create device token
    let device_token = authority.create_device_token(device_id).unwrap();
    
    // Create authorization bridge with wrong public key
    let wrong_key = PublicKey::from_bytes(&[0u8; 32]).unwrap();
    let bridge = BiscuitAuthorizationBridge::new(wrong_key, device_id);
    
    // Create resource scope
    let authority_id = AuthorityId::new();
    let resource_scope = ResourceScope::Authority {
        authority_id,
        operation: AuthorityOp::AddDevice,
    };
    
    // Test authorization - should fail due to signature verification failure
    let result = bridge.authorize(&device_token, "write", &resource_scope);
    assert!(result.is_err(), "Authorization with wrong public key should fail");
}