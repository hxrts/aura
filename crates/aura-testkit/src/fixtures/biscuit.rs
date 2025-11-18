//! Biscuit token fixtures for testing authorization scenarios
//!
//! This module provides comprehensive test fixtures for creating and managing
//! Biscuit tokens of different types (device, guardian, delegated) with various
//! authorization scenarios and delegation chains.

use aura_core::{AccountId, DeviceId};
use aura_wot::{
    biscuit_resources::{ResourceScope, StorageCategory},
    biscuit_token::{AccountAuthority, BiscuitError, BiscuitTokenManager},
};
use biscuit_auth::{macros::*, Biscuit, PublicKey};
use std::collections::HashMap;
use std::time::SystemTime;

/// Comprehensive fixture for Biscuit token testing scenarios
pub struct BiscuitTestFixture {
    pub account_authority: AccountAuthority,
    pub device_tokens: HashMap<DeviceId, BiscuitTokenManager>,
    pub guardian_tokens: HashMap<DeviceId, Biscuit>,
    pub delegated_tokens: Vec<DelegatedTokenChain>,
}

/// Represents a chain of token delegations for testing attenuation
pub struct DelegatedTokenChain {
    pub chain_id: String,
    pub original_token: Biscuit,
    pub delegated_tokens: Vec<Biscuit>,
    pub resource_scopes: Vec<ResourceScope>,
}

impl BiscuitTestFixture {
    /// Create a new test fixture with a random account
    pub fn new() -> Self {
        let account_id = AccountId::new();
        let account_authority = AccountAuthority::new(account_id);

        Self {
            account_authority,
            device_tokens: HashMap::new(),
            guardian_tokens: HashMap::new(),
            delegated_tokens: Vec::new(),
        }
    }

    /// Create a new test fixture with a specific account ID
    pub fn with_account(account_id: AccountId) -> Self {
        let account_authority = AccountAuthority::new(account_id);

        Self {
            account_authority,
            device_tokens: HashMap::new(),
            guardian_tokens: HashMap::new(),
            delegated_tokens: Vec::new(),
        }
    }

    /// Add a device token with full owner capabilities
    pub fn add_device_token(&mut self, device_id: DeviceId) -> Result<(), BiscuitError> {
        let token = self.account_authority.create_device_token(device_id)?;
        let manager = BiscuitTokenManager::new(device_id, token);
        self.device_tokens.insert(device_id, manager);
        Ok(())
    }

    /// Add a guardian token with recovery-specific capabilities
    pub fn add_guardian_token(&mut self, device_id: DeviceId) -> Result<(), BiscuitError> {
        let account = self.account_authority.account_id().to_string();
        let device = device_id.to_string();

        let guardian_token = biscuit!(
            r#"
            account({account});
            device({device});
            role("guardian");
            capability("read");
            capability("recovery_initiate");
            capability("recovery_approve");
            capability("threshold_sign");

            // Guardian-specific constraints
            check if operation($op), ["read", "recovery_initiate", "recovery_approve", "threshold_sign"].contains($op);
            check if resource($res), $res.starts_with("/recovery/") || $res.starts_with("/journal/");
        "#
        )
        .build(self.account_authority.root_keypair())?;

        self.guardian_tokens.insert(device_id, guardian_token);
        Ok(())
    }

    /// Create a delegated token chain with progressive attenuation
    pub fn create_delegation_chain(
        &mut self,
        chain_id: &str,
        source_device: DeviceId,
        resource_scopes: Vec<ResourceScope>,
    ) -> Result<(), BiscuitError> {
        let source_manager = self.device_tokens.get(&source_device).ok_or_else(|| {
            BiscuitError::AuthorizationFailed("Source device not found".to_string())
        })?;

        let original_token = source_manager.current_token().clone();
        let mut delegated_tokens = Vec::new();
        let mut current_token = original_token.clone();

        // Create progressive attenuation chain
        for (index, scope) in resource_scopes.iter().enumerate() {
            let index_i64 = index as i64;
            let attenuated_token = match scope {
                ResourceScope::Storage { category, path } => {
                    let resource_pattern = scope.resource_pattern();
                    let category_str = category.as_str();
                    current_token.append(block!(
                        r#"
                        check if operation($op), ["read"].contains($op);
                        check if resource($res), $res.starts_with({resource_pattern});
                        check if storage_category({category_str});

                        // Add delegation depth tracking
                        delegation_depth({index_i64});
                    "#
                    ))?
                }
                ResourceScope::Journal {
                    account_id,
                    operation,
                } => {
                    let account_id_str = account_id.clone();
                    let op_str = operation.as_str();
                    current_token.append(block!(
                        r#"
                        check if operation({op_str});
                        check if account({account_id_str});
                        check if resource($res), $res.starts_with("/journal/");

                        delegation_depth({index_i64});
                    "#
                    ))?
                }
                ResourceScope::Relay { channel_id } => {
                    let channel = channel_id.clone();
                    current_token.append(block!(
                        r#"
                        check if operation($op), ["relay_message"].contains($op);
                        check if channel({channel});

                        delegation_depth({index_i64});
                    "#
                    ))?
                }
                ResourceScope::Recovery { recovery_type } => {
                    let recovery_type_str = recovery_type.as_str();
                    current_token.append(block!(
                        r#"
                        check if operation($op), ["recovery_approve", "threshold_sign"].contains($op);
                        check if recovery_type({recovery_type_str});

                        delegation_depth({index_i64});
                    "#
                    ))?
                }
                ResourceScope::Admin { operation } => {
                    let admin_op = operation.as_str();
                    current_token.append(block!(
                        r#"
                        check if operation({admin_op});
                        check if role($role), ["admin", "owner"].contains($role);

                        delegation_depth({index_i64});
                    "#
                    ))?
                }
            };

            delegated_tokens.push(attenuated_token.clone());
            current_token = attenuated_token;
        }

        let chain = DelegatedTokenChain {
            chain_id: chain_id.to_string(),
            original_token,
            delegated_tokens,
            resource_scopes,
        };

        self.delegated_tokens.push(chain);
        Ok(())
    }

    /// Create a token with time-based expiration
    pub fn create_expiring_token(
        &self,
        device_id: DeviceId,
        expiration_seconds: u64,
    ) -> Result<Biscuit, BiscuitError> {
        let account = self.account_authority.account_id().to_string();
        let device = device_id.to_string();
        let expiry_time = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64
            + expiration_seconds as i64;

        let expiring_token = biscuit!(
            r#"
            account({account});
            device({device});
            role("temporary");
            capability("read");

            // Time-based expiration
            check if time($time), $time < {expiry_time};
        "#
        )
        .build(self.account_authority.root_keypair())?;

        Ok(expiring_token)
    }

    /// Create a token with delegation depth limit
    pub fn create_depth_limited_token(
        &self,
        device_id: DeviceId,
        max_depth: u32,
    ) -> Result<Biscuit, BiscuitError> {
        let account = self.account_authority.account_id().to_string();
        let device = device_id.to_string();
        let max_depth_i64 = max_depth as i64;

        let depth_limited_token = biscuit!(
            r#"
            account({account});
            device({device});
            role("delegator");
            capability("read");
            capability("delegate");

            // Delegation depth limit
            delegation_max_depth({max_depth_i64});
            check if delegation_depth($depth), $depth <= {max_depth_i64};
        "#
        )
        .build(self.account_authority.root_keypair())?;

        Ok(depth_limited_token)
    }

    /// Get the root public key for token verification
    pub fn root_public_key(&self) -> PublicKey {
        self.account_authority.root_public_key()
    }

    /// Get a device token manager
    pub fn get_device_token(&self, device_id: &DeviceId) -> Option<&BiscuitTokenManager> {
        self.device_tokens.get(device_id)
    }

    /// Get a guardian token
    pub fn get_guardian_token(&self, device_id: &DeviceId) -> Option<&Biscuit> {
        self.guardian_tokens.get(device_id)
    }

    /// Get a delegation chain by ID
    pub fn get_delegation_chain(&self, chain_id: &str) -> Option<&DelegatedTokenChain> {
        self.delegated_tokens
            .iter()
            .find(|chain| chain.chain_id == chain_id)
    }

    /// Create a token with minimal privileges (for testing privilege escalation prevention)
    pub fn create_minimal_token(&self, device_id: DeviceId) -> Result<Biscuit, BiscuitError> {
        let account = self.account_authority.account_id().to_string();
        let device = device_id.to_string();

        let minimal_token = biscuit!(
            r#"
            account({account});
            device({device});
            role("read_only");

            // Very restricted capabilities
            capability("read");

            // Only allow reading from specific paths
            check if operation("read");
            check if resource($res), $res.starts_with("/storage/personal/read_only/");
        "#
        )
        .build(self.account_authority.root_keypair())?;

        Ok(minimal_token)
    }

    /// Create a compromised token scenario for security testing
    pub fn create_compromised_scenario(
        &self,
        device_id: DeviceId,
    ) -> Result<Biscuit, BiscuitError> {
        let account = self.account_authority.account_id().to_string();
        let device = device_id.to_string();

        // Create a token that might be used in privilege escalation attempts
        let suspicious_token = biscuit!(
            r#"
            account({account});
            device({device});
            role("suspicious");
            capability("read");

            // This token will be used to test privilege escalation prevention
            check if operation("read");
            check if resource($res), $res.starts_with("/storage/public/");

            // Add suspicious facts that shouldn't grant additional privileges
            compromised_device(true);
            attempted_privilege_escalation(true);
        "#
        )
        .build(self.account_authority.root_keypair())?;

        Ok(suspicious_token)
    }

    /// Get account ID for this fixture
    pub fn account_id(&self) -> AccountId {
        self.account_authority.account_id()
    }
}

impl Default for BiscuitTestFixture {
    fn default() -> Self {
        Self::new()
    }
}

/// Convenience functions for creating common test scenarios

/// Create a basic multi-device scenario with owner and guardian
pub fn create_multi_device_scenario() -> Result<BiscuitTestFixture, BiscuitError> {
    let mut fixture = BiscuitTestFixture::new();

    // Add owner device
    let owner_device = DeviceId::new();
    fixture.add_device_token(owner_device)?;

    // Add two guardians
    let guardian1 = DeviceId::new();
    let guardian2 = DeviceId::new();
    fixture.add_guardian_token(guardian1)?;
    fixture.add_guardian_token(guardian2)?;

    Ok(fixture)
}

/// Create a delegation chain scenario with progressive attenuation
pub fn create_delegation_scenario() -> Result<BiscuitTestFixture, BiscuitError> {
    let mut fixture = BiscuitTestFixture::new();

    let owner_device = DeviceId::new();
    fixture.add_device_token(owner_device)?;

    // Create a delegation chain with progressive restrictions
    let resource_scopes = vec![
        ResourceScope::Storage {
            category: StorageCategory::Personal,
            path: "documents/".to_string(),
        },
        ResourceScope::Storage {
            category: StorageCategory::Personal,
            path: "documents/public/".to_string(),
        },
        ResourceScope::Storage {
            category: StorageCategory::Personal,
            path: "documents/public/readonly/".to_string(),
        },
    ];

    fixture.create_delegation_chain("progressive_restriction", owner_device, resource_scopes)?;

    Ok(fixture)
}

/// Create a recovery scenario with guardian tokens
pub fn create_recovery_scenario() -> Result<BiscuitTestFixture, BiscuitError> {
    let mut fixture = BiscuitTestFixture::new();

    // Add compromised device (to be recovered)
    let compromised_device = DeviceId::new();
    fixture.add_device_token(compromised_device)?;

    // Add three guardians for 2-of-3 recovery
    for _ in 0..3 {
        let guardian_device = DeviceId::new();
        fixture.add_guardian_token(guardian_device)?;
    }

    Ok(fixture)
}

/// Create a security testing scenario with various privilege levels
pub fn create_security_test_scenario() -> Result<BiscuitTestFixture, BiscuitError> {
    let mut fixture = BiscuitTestFixture::new();

    // Add devices with different privilege levels
    let admin_device = DeviceId::new();
    let regular_device = DeviceId::new();
    let restricted_device = DeviceId::new();

    fixture.add_device_token(admin_device)?;
    fixture.add_device_token(regular_device)?;

    // Create restricted and compromised tokens for security testing
    let _minimal_token = fixture.create_minimal_token(restricted_device)?;
    let _compromised_token = fixture.create_compromised_scenario(DeviceId::new())?;

    Ok(fixture)
}

// AccountAuthorityExt is no longer needed since AccountAuthority.account_id() is now available

// Extension traits are no longer needed since as_str methods are now public on the original types

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_fixture_creation() {
        let fixture = BiscuitTestFixture::new();
        assert!(fixture.device_tokens.is_empty());
        assert!(fixture.guardian_tokens.is_empty());
        assert!(fixture.delegated_tokens.is_empty());
    }

    #[test]
    fn test_device_token_creation() {
        let mut fixture = BiscuitTestFixture::new();
        let device_id = DeviceId::new();

        fixture.add_device_token(device_id).unwrap();
        assert!(fixture.device_tokens.contains_key(&device_id));
    }

    #[test]
    fn test_guardian_token_creation() {
        let mut fixture = BiscuitTestFixture::new();
        let device_id = DeviceId::new();

        fixture.add_guardian_token(device_id).unwrap();
        assert!(fixture.guardian_tokens.contains_key(&device_id));
    }

    #[test]
    fn test_multi_device_scenario() {
        let fixture = create_multi_device_scenario().unwrap();
        assert!(!fixture.device_tokens.is_empty());
        assert!(!fixture.guardian_tokens.is_empty());
    }

    #[test]
    fn test_delegation_scenario() {
        let fixture = create_delegation_scenario().unwrap();
        assert!(!fixture.device_tokens.is_empty());
        assert!(!fixture.delegated_tokens.is_empty());
    }

    #[test]
    fn test_recovery_scenario() {
        let fixture = create_recovery_scenario().unwrap();
        assert!(!fixture.device_tokens.is_empty());
        assert!(!fixture.guardian_tokens.is_empty());
        assert_eq!(fixture.guardian_tokens.len(), 3);
    }

    #[test]
    fn test_security_test_scenario() {
        let fixture = create_security_test_scenario().unwrap();
        assert!(!fixture.device_tokens.is_empty());
    }

    #[test]
    fn test_expiring_token_creation() {
        let fixture = BiscuitTestFixture::new();
        let device_id = DeviceId::new();

        let token = fixture.create_expiring_token(device_id, 3600).unwrap();
        assert!(!token.to_vec().unwrap().is_empty());
    }

    #[test]
    fn test_depth_limited_token_creation() {
        let fixture = BiscuitTestFixture::new();
        let device_id = DeviceId::new();

        let token = fixture.create_depth_limited_token(device_id, 3).unwrap();
        assert!(!token.to_vec().unwrap().is_empty());
    }
}
