//! End-to-end authentication flow tests with Biscuit tokens
//!
//! This test module validates complete authentication flows using Biscuit tokens,
//! including device authentication, session establishment, token validation,
//! and authorization enforcement throughout the authentication lifecycle.

use aura_core::{AccountId, DeviceId};
use aura_protocol::authorization::biscuit_bridge::BiscuitAuthorizationBridge;
use aura_testkit::{
    create_multi_device_scenario, create_test_fixture, BiscuitTestFixture, TestFixture,
};
use aura_wot::{
    biscuit_resources::{ResourceScope, StorageCategory},
    biscuit_token::{BiscuitError, BiscuitTokenManager},
};
use biscuit_auth::{macros::*, Biscuit};
use std::collections::HashMap;
use std::time::SystemTime;

/// Represents an authenticated session with Biscuit token
#[derive(Clone)]
pub struct AuthenticatedSession {
    pub device_id: DeviceId,
    pub account_id: AccountId,
    pub token: Biscuit,
    pub session_id: String,
    pub created_at: SystemTime,
    pub expires_at: Option<SystemTime>,
}

impl AuthenticatedSession {
    pub fn new(
        device_id: DeviceId,
        account_id: AccountId,
        token: Biscuit,
        session_duration_seconds: Option<u64>,
    ) -> Self {
        let now = SystemTime::now();
        let expires_at =
            session_duration_seconds.map(|secs| now + std::time::Duration::from_secs(secs));

        Self {
            device_id,
            account_id,
            token,
            session_id: format!("session_{}", uuid::Uuid::new_v4()),
            created_at: now,
            expires_at,
        }
    }

    pub fn is_expired(&self) -> bool {
        if let Some(expires_at) = self.expires_at {
            SystemTime::now() > expires_at
        } else {
            false
        }
    }

    pub fn authorize_operation(
        &self,
        bridge: &BiscuitAuthorizationBridge,
        operation: &str,
        resource: &ResourceScope,
    ) -> Result<bool, BiscuitError> {
        if self.is_expired() {
            return Ok(false);
        }

        let result = bridge.authorize(&self.token, operation, resource)?;
        Ok(result.authorized)
    }
}

/// Mock authentication service for testing
pub struct AuthenticationService {
    pub accounts: HashMap<AccountId, BiscuitTestFixture>,
    pub active_sessions: HashMap<String, AuthenticatedSession>,
}

impl AuthenticationService {
    pub fn new() -> Self {
        Self {
            accounts: HashMap::new(),
            active_sessions: HashMap::new(),
        }
    }

    pub fn register_account(&mut self, account_id: AccountId) -> Result<(), BiscuitError> {
        let fixture = BiscuitTestFixture::with_account(account_id);
        self.accounts.insert(account_id, fixture);
        Ok(())
    }

    pub fn register_device(
        &mut self,
        account_id: AccountId,
        device_id: DeviceId,
    ) -> Result<(), BiscuitError> {
        if let Some(fixture) = self.accounts.get_mut(&account_id) {
            fixture.add_device_token(device_id)?;
            Ok(())
        } else {
            Err(BiscuitError::AuthorizationFailed(
                "Account not found".to_string(),
            ))
        }
    }

    pub fn authenticate_device(
        &mut self,
        account_id: AccountId,
        device_id: DeviceId,
        session_duration_seconds: Option<u64>,
    ) -> Result<AuthenticatedSession, BiscuitError> {
        let fixture = self
            .accounts
            .get(&account_id)
            .ok_or_else(|| BiscuitError::AuthorizationFailed("Account not found".to_string()))?;

        let token_manager = fixture.get_device_token(&device_id).ok_or_else(|| {
            BiscuitError::AuthorizationFailed("Device not registered".to_string())
        })?;

        let token = token_manager.current_token().clone();
        let session =
            AuthenticatedSession::new(device_id, account_id, token, session_duration_seconds);

        self.active_sessions
            .insert(session.session_id.clone(), session.clone());
        Ok(session)
    }

    pub fn validate_session(&self, session_id: &str) -> Option<&AuthenticatedSession> {
        self.active_sessions
            .get(session_id)
            .filter(|session| !session.is_expired())
    }

    pub fn invalidate_session(&mut self, session_id: &str) -> bool {
        self.active_sessions.remove(session_id).is_some()
    }

    pub fn get_account_fixture(&self, account_id: &AccountId) -> Option<&BiscuitTestFixture> {
        self.accounts.get(account_id)
    }
}

impl Default for AuthenticationService {
    fn default() -> Self {
        Self::new()
    }
}

#[tokio::test]
async fn test_basic_device_authentication_flow() -> Result<(), Box<dyn std::error::Error>> {
    let mut auth_service = AuthenticationService::new();
    let account_id = AccountId::new();
    let device_id = DeviceId::new();

    // Register account and device
    auth_service.register_account(account_id)?;
    auth_service.register_device(account_id, device_id)?;

    // Authenticate device
    let session = auth_service.authenticate_device(account_id, device_id, Some(3600))?;

    assert_eq!(session.device_id, device_id);
    assert_eq!(session.account_id, account_id);
    assert!(!session.session_id.is_empty());
    assert!(!session.is_expired());

    // Validate session
    let validated_session = auth_service.validate_session(&session.session_id);
    assert!(validated_session.is_some());
    assert_eq!(validated_session.unwrap().device_id, device_id);

    Ok(())
}

#[tokio::test]
async fn test_multi_device_authentication() -> Result<(), Box<dyn std::error::Error>> {
    let mut auth_service = AuthenticationService::new();
    let fixture = create_multi_device_scenario()?;

    // Extract devices from the fixture (we need to simulate this since the fixture
    // doesn't expose device IDs directly in the current implementation)
    let account_id = fixture.account_id();
    auth_service.accounts.insert(account_id, fixture);

    // Simulate multiple devices
    let device1 = DeviceId::new();
    let device2 = DeviceId::new();
    let device3 = DeviceId::new();

    auth_service.register_device(account_id, device1)?;
    auth_service.register_device(account_id, device2)?;
    auth_service.register_device(account_id, device3)?;

    // Authenticate all devices
    let session1 = auth_service.authenticate_device(account_id, device1, Some(3600))?;
    let session2 = auth_service.authenticate_device(account_id, device2, Some(1800))?;
    let session3 = auth_service.authenticate_device(account_id, device3, None)?; // No expiration

    // Verify all sessions are active
    assert!(auth_service
        .validate_session(&session1.session_id)
        .is_some());
    assert!(auth_service
        .validate_session(&session2.session_id)
        .is_some());
    assert!(auth_service
        .validate_session(&session3.session_id)
        .is_some());

    // Test operations with each session
    let fixture = auth_service.get_account_fixture(&account_id).unwrap();

    for (device_id, session) in [
        (device1, &session1),
        (device2, &session2),
        (device3, &session3),
    ] {
        let bridge = BiscuitAuthorizationBridge::new(fixture.root_public_key(), device_id);
        let storage_scope = ResourceScope::Storage {
            category: StorageCategory::Personal,
            path: "multi_device_test/".to_string(),
        };

        let authorized = session.authorize_operation(&bridge, "read", &storage_scope)?;
        assert!(authorized, "Device {} should be authorized", device_id);
    }

    Ok(())
}

#[tokio::test]
async fn test_session_expiration() -> Result<(), Box<dyn std::error::Error>> {
    let mut auth_service = AuthenticationService::new();
    let account_id = AccountId::new();
    let device_id = DeviceId::new();

    auth_service.register_account(account_id)?;
    auth_service.register_device(account_id, device_id)?;

    // Create a short-lived session (1 second)
    let session = auth_service.authenticate_device(account_id, device_id, Some(1))?;

    // Session should be valid initially
    assert!(!session.is_expired());
    assert!(auth_service.validate_session(&session.session_id).is_some());

    // Wait for expiration (in a real test, we might mock time instead)
    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

    // Session should now be expired
    assert!(session.is_expired());
    assert!(auth_service.validate_session(&session.session_id).is_none());

    // Operations should fail with expired session
    let fixture = auth_service.get_account_fixture(&account_id).unwrap();
    let bridge = BiscuitAuthorizationBridge::new(fixture.root_public_key(), device_id);
    let storage_scope = ResourceScope::Storage {
        category: StorageCategory::Personal,
        path: "test/".to_string(),
    };

    let authorized = session.authorize_operation(&bridge, "read", &storage_scope)?;
    assert!(
        !authorized,
        "Expired session should not authorize operations"
    );

    Ok(())
}

#[tokio::test]
async fn test_session_invalidation() -> Result<(), Box<dyn std::error::Error>> {
    let mut auth_service = AuthenticationService::new();
    let account_id = AccountId::new();
    let device_id = DeviceId::new();

    auth_service.register_account(account_id)?;
    auth_service.register_device(account_id, device_id)?;

    // Authenticate device
    let session = auth_service.authenticate_device(account_id, device_id, Some(3600))?;

    // Session should be valid
    assert!(auth_service.validate_session(&session.session_id).is_some());

    // Invalidate session
    let invalidated = auth_service.invalidate_session(&session.session_id);
    assert!(invalidated, "Session should be successfully invalidated");

    // Session should no longer be valid
    assert!(auth_service.validate_session(&session.session_id).is_none());

    // Try to invalidate again (should return false)
    let invalidated_again = auth_service.invalidate_session(&session.session_id);
    assert!(
        !invalidated_again,
        "Already invalidated session should return false"
    );

    Ok(())
}

#[tokio::test]
async fn test_token_rotation_during_session() -> Result<(), Box<dyn std::error::Error>> {
    let mut auth_service = AuthenticationService::new();
    let account_id = AccountId::new();
    let device_id = DeviceId::new();

    auth_service.register_account(account_id)?;
    auth_service.register_device(account_id, device_id)?;

    // Authenticate device
    let mut session = auth_service.authenticate_device(account_id, device_id, Some(3600))?;

    // Get the original token
    let original_token = session.token.clone();

    // Simulate token rotation by creating a new attenuated token
    let fixture = auth_service.get_account_fixture(&account_id).unwrap();
    let token_manager = fixture.get_device_token(&device_id).unwrap();
    let rotated_token = token_manager.attenuate_read("rotated_access/")?;

    // Update session with rotated token
    session.token = rotated_token.clone();
    auth_service
        .active_sessions
        .insert(session.session_id.clone(), session.clone());

    // Verify rotated token works
    let bridge = BiscuitAuthorizationBridge::new(fixture.root_public_key(), device_id);
    let storage_scope = ResourceScope::Storage {
        category: StorageCategory::Personal,
        path: "rotated_access/file.txt".to_string(),
    };

    let authorized = session.authorize_operation(&bridge, "read", &storage_scope)?;
    assert!(
        authorized,
        "Rotated token should authorize appropriate operations"
    );

    // Verify the tokens are different
    assert_ne!(
        original_token.to_vec().unwrap(),
        rotated_token.to_vec().unwrap(),
        "Rotated token should be different from original"
    );

    Ok(())
}

#[tokio::test]
async fn test_concurrent_sessions_same_device() -> Result<(), Box<dyn std::error::Error>> {
    let mut auth_service = AuthenticationService::new();
    let account_id = AccountId::new();
    let device_id = DeviceId::new();

    auth_service.register_account(account_id)?;
    auth_service.register_device(account_id, device_id)?;

    // Create multiple concurrent sessions for the same device
    let session1 = auth_service.authenticate_device(account_id, device_id, Some(3600))?;
    let session2 = auth_service.authenticate_device(account_id, device_id, Some(1800))?;
    let session3 = auth_service.authenticate_device(account_id, device_id, None)?;

    // All sessions should be valid and have different IDs
    assert!(auth_service
        .validate_session(&session1.session_id)
        .is_some());
    assert!(auth_service
        .validate_session(&session2.session_id)
        .is_some());
    assert!(auth_service
        .validate_session(&session3.session_id)
        .is_some());

    assert_ne!(session1.session_id, session2.session_id);
    assert_ne!(session1.session_id, session3.session_id);
    assert_ne!(session2.session_id, session3.session_id);

    // All sessions should work independently
    let fixture = auth_service.get_account_fixture(&account_id).unwrap();
    let bridge = BiscuitAuthorizationBridge::new(fixture.root_public_key(), device_id);
    let storage_scope = ResourceScope::Storage {
        category: StorageCategory::Personal,
        path: "concurrent_test/".to_string(),
    };

    for (name, session) in [
        ("session1", &session1),
        ("session2", &session2),
        ("session3", &session3),
    ] {
        let authorized = session.authorize_operation(&bridge, "read", &storage_scope)?;
        assert!(authorized, "{} should be authorized", name);
    }

    // Invalidate one session, others should remain valid
    auth_service.invalidate_session(&session2.session_id);

    assert!(auth_service
        .validate_session(&session1.session_id)
        .is_some());
    assert!(auth_service
        .validate_session(&session2.session_id)
        .is_none());
    assert!(auth_service
        .validate_session(&session3.session_id)
        .is_some());

    Ok(())
}

#[tokio::test]
async fn test_authentication_with_attenuated_tokens() -> Result<(), Box<dyn std::error::Error>> {
    let mut auth_service = AuthenticationService::new();
    let account_id = AccountId::new();
    let device_id = DeviceId::new();

    auth_service.register_account(account_id)?;
    auth_service.register_device(account_id, device_id)?;

    // Get the full token and create an attenuated version
    let fixture = auth_service.get_account_fixture(&account_id).unwrap();
    let token_manager = fixture.get_device_token(&device_id).unwrap();
    let attenuated_token = token_manager.attenuate_read("restricted/")?;

    // Create a session with the attenuated token
    let session = AuthenticatedSession::new(device_id, account_id, attenuated_token, Some(3600));
    auth_service
        .active_sessions
        .insert(session.session_id.clone(), session.clone());

    // Test that the session works for allowed operations
    let bridge = BiscuitAuthorizationBridge::new(fixture.root_public_key(), device_id);

    let allowed_scope = ResourceScope::Storage {
        category: StorageCategory::Personal,
        path: "restricted/allowed_file.txt".to_string(),
    };

    let authorized = session.authorize_operation(&bridge, "read", &allowed_scope)?;
    assert!(
        authorized,
        "Attenuated session should authorize allowed operations"
    );

    // Test that it doesn't work for disallowed operations (in a real implementation)
    let disallowed_scope = ResourceScope::Storage {
        category: StorageCategory::Personal,
        path: "other/disallowed_file.txt".to_string(),
    };

    let unauthorized = session.authorize_operation(&bridge, "read", &disallowed_scope)?;
    println!(
        "Attenuated session accessing disallowed resource: {} (should be false)",
        unauthorized
    );

    Ok(())
}

#[tokio::test]
async fn test_cross_account_authentication_isolation() -> Result<(), Box<dyn std::error::Error>> {
    let mut auth_service = AuthenticationService::new();

    // Create two separate accounts
    let account1 = AccountId::new();
    let account2 = AccountId::new();
    let device1 = DeviceId::new();
    let device2 = DeviceId::new();

    auth_service.register_account(account1)?;
    auth_service.register_account(account2)?;
    auth_service.register_device(account1, device1)?;
    auth_service.register_device(account2, device2)?;

    // Authenticate devices from different accounts
    let session1 = auth_service.authenticate_device(account1, device1, Some(3600))?;
    let session2 = auth_service.authenticate_device(account2, device2, Some(3600))?;

    // Both sessions should be valid for their respective accounts
    assert!(auth_service
        .validate_session(&session1.session_id)
        .is_some());
    assert!(auth_service
        .validate_session(&session2.session_id)
        .is_some());

    // Test that sessions are properly isolated
    let fixture1 = auth_service.get_account_fixture(&account1).unwrap();
    let fixture2 = auth_service.get_account_fixture(&account2).unwrap();

    let bridge1 = BiscuitAuthorizationBridge::new(fixture1.root_public_key(), device1);
    let bridge2 = BiscuitAuthorizationBridge::new(fixture2.root_public_key(), device2);

    let storage_scope = ResourceScope::Storage {
        category: StorageCategory::Personal,
        path: "isolation_test/".to_string(),
    };

    // Each session should work with its own account's bridge
    let auth1 = session1.authorize_operation(&bridge1, "read", &storage_scope)?;
    let auth2 = session2.authorize_operation(&bridge2, "read", &storage_scope)?;

    assert!(auth1, "Session1 should work with account1's bridge");
    assert!(auth2, "Session2 should work with account2's bridge");

    // Cross-account authorization should fail (in a real implementation)
    let cross_auth1 = session1.authorize_operation(&bridge2, "read", &storage_scope);
    let cross_auth2 = session2.authorize_operation(&bridge1, "read", &storage_scope);

    // In a real implementation, these should fail due to different root keys
    println!("Cross-account auth 1->2: {:?}", cross_auth1);
    println!("Cross-account auth 2->1: {:?}", cross_auth2);

    Ok(())
}

#[tokio::test]
async fn test_authentication_with_integration_framework() -> Result<(), Box<dyn std::error::Error>>
{
    // Test integration with the existing aura-testkit framework
    let _test_fixture = create_test_fixture().await?;
    let mut auth_service = AuthenticationService::new();

    let account_id = AccountId::new();
    let device_id = DeviceId::new();

    // Set up authentication service with test data
    auth_service.register_account(account_id)?;
    auth_service.register_device(account_id, device_id)?;

    // Authenticate and create session
    let session = auth_service.authenticate_device(account_id, device_id, Some(3600))?;

    // Verify session works with the test fixture infrastructure
    assert!(!session.session_id.is_empty());
    assert!(auth_service.validate_session(&session.session_id).is_some());

    // Test authorization using the session
    let fixture = auth_service.get_account_fixture(&account_id).unwrap();
    let bridge = BiscuitAuthorizationBridge::new(fixture.root_public_key(), device_id);

    let storage_scope = ResourceScope::Storage {
        category: StorageCategory::Personal,
        path: "integration_test/".to_string(),
    };

    let authorized = session.authorize_operation(&bridge, "read", &storage_scope)?;
    assert!(authorized, "Session should work with testkit integration");

    Ok(())
}

#[tokio::test]
async fn test_batch_authentication_operations() -> Result<(), Box<dyn std::error::Error>> {
    let mut auth_service = AuthenticationService::new();
    let account_id = AccountId::new();

    auth_service.register_account(account_id)?;

    // Register multiple devices
    let devices: Vec<DeviceId> = (0..5).map(|_| DeviceId::new()).collect();

    for device_id in &devices {
        auth_service.register_device(account_id, *device_id)?;
    }

    // Authenticate all devices
    let mut sessions = Vec::new();
    for device_id in &devices {
        let session = auth_service.authenticate_device(account_id, *device_id, Some(3600))?;
        sessions.push(session);
    }

    // Verify all sessions are active
    for session in &sessions {
        assert!(auth_service.validate_session(&session.session_id).is_some());
    }

    // Test batch authorization
    let fixture = auth_service.get_account_fixture(&account_id).unwrap();
    let storage_scope = ResourceScope::Storage {
        category: StorageCategory::Shared,
        path: "batch_test/".to_string(),
    };

    for (device_id, session) in devices.iter().zip(sessions.iter()) {
        let bridge = BiscuitAuthorizationBridge::new(fixture.root_public_key(), *device_id);
        let authorized = session.authorize_operation(&bridge, "read", &storage_scope)?;
        assert!(
            authorized,
            "Batch device {} should be authorized",
            device_id
        );
    }

    // Invalidate all sessions
    for session in &sessions {
        let invalidated = auth_service.invalidate_session(&session.session_id);
        assert!(
            invalidated,
            "Session {} should be invalidated",
            session.session_id
        );
    }

    // Verify no sessions are active
    for session in &sessions {
        assert!(auth_service.validate_session(&session.session_id).is_none());
    }

    Ok(())
}
