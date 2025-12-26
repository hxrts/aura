//! Biscuit token fixtures for testing authorization scenarios
//!
//! This module provides comprehensive test fixtures for creating and managing
//! Biscuit tokens of different types (authority, guardian, delegated) with various
//! authorization scenarios and delegation chains.
//!
//! **Authority Model**: Uses authority-centric identity model where authorities
//! (not devices) are the cryptographic actors that issue and manage tokens.

use aura_core::identifiers::AuthorityId;
use aura_core::scope::{AuthorityOp, ResourceScope};
use aura_authorization::biscuit_token::{BiscuitError, BiscuitTokenManager, TokenAuthority};
use biscuit_auth::{macros::*, Biscuit, PublicKey};
use std::collections::HashMap;
use std::time::SystemTime;

fn authority(seed: u8) -> AuthorityId {
    AuthorityId::new_from_entropy([seed; 32])
}

/// Comprehensive fixture for Biscuit token testing scenarios
///
/// **Authority Model**: Tokens are issued by authorities to other authorities.
/// This replaces the legacy device-centric model.
pub struct BiscuitTestFixture {
    pub token_authority: TokenAuthority,
    pub authority_tokens: HashMap<AuthorityId, BiscuitTokenManager>,
    pub guardian_tokens: HashMap<AuthorityId, Biscuit>,
    pub delegated_tokens: Vec<DelegatedTokenChain>,
}

/// Represents a chain of token delegations for testing attenuation
#[derive(Clone)]
pub struct DelegatedTokenChain {
    pub chain_id: String,
    pub original_token: Biscuit,
    pub delegated_tokens: Vec<Biscuit>,
    pub resource_scopes: Vec<ResourceScope>,
}

impl BiscuitTestFixture {
    /// Create a new test fixture with a random authority
    pub fn new() -> Self {
        let authority_id = AuthorityId::new_from_entropy([0u8; 32]);
        let token_authority = TokenAuthority::new(authority_id);

        Self {
            token_authority,
            authority_tokens: HashMap::new(),
            guardian_tokens: HashMap::new(),
            delegated_tokens: Vec::new(),
        }
    }

    /// Create a new test fixture with a specific authority ID
    pub fn with_authority(authority_id: AuthorityId) -> Self {
        let token_authority = TokenAuthority::new(authority_id);

        Self {
            token_authority,
            authority_tokens: HashMap::new(),
            guardian_tokens: HashMap::new(),
            delegated_tokens: Vec::new(),
        }
    }

    /// Add an authority token with full owner capabilities
    pub fn add_authority_token(&mut self, recipient: AuthorityId) -> Result<(), BiscuitError> {
        let token = self.token_authority.create_token(recipient)?;
        let manager = BiscuitTokenManager::new(recipient, token);
        self.authority_tokens.insert(recipient, manager);
        Ok(())
    }

    /// Add a guardian token with recovery-specific capabilities
    pub fn add_guardian_token(
        &mut self,
        guardian_authority: AuthorityId,
    ) -> Result<(), BiscuitError> {
        let issuer = self.token_authority.authority_id().to_string();
        let guardian = guardian_authority.to_string();

        let guardian_token = biscuit!(
            r#"
            issuer({issuer});
            authority({guardian});
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
        .build(self.token_authority.root_keypair())?;

        self.guardian_tokens
            .insert(guardian_authority, guardian_token);
        Ok(())
    }

    /// Create a delegated token chain with progressive attenuation
    pub fn create_delegation_chain(
        &mut self,
        chain_id: &str,
        source_authority: AuthorityId,
        resource_scopes: Vec<ResourceScope>,
    ) -> Result<(), BiscuitError> {
        let source_manager = self
            .authority_tokens
            .get(&source_authority)
            .ok_or_else(|| {
                BiscuitError::AuthorizationFailed("Source authority not found".to_string())
            })?;

        let original_token = source_manager.current_token().clone();
        let mut delegated_tokens = Vec::new();
        let mut current_token = original_token.clone();

        // Create progressive attenuation chain
        for (index, scope) in resource_scopes.iter().enumerate() {
            let index_i64 = index as i64;
            let attenuated_token = match scope {
                ResourceScope::Authority {
                    authority_id,
                    operation,
                } => {
                    let authority_str = authority_id.to_string();
                    let op_str = format!("{:?}", operation);
                    current_token.append(block!(
                        r#"
                        check if authority({authority_str});
                        check if operation({op_str});

                        // Add delegation depth tracking
                        delegation_depth({index_i64});
                    "#
                    ))?
                }
                ResourceScope::Context {
                    context_id,
                    operation,
                } => {
                    let context_str = context_id.to_string();
                    let op_str = format!("{:?}", operation);
                    current_token.append(block!(
                        r#"
                        check if context({context_str});
                        check if operation({op_str});

                        delegation_depth({index_i64});
                    "#
                    ))?
                }
                ResourceScope::Storage { authority_id, path } => {
                    let authority_str = authority_id.to_string();
                    let path_str = path.clone();
                    current_token.append(block!(
                        r#"
                        check if authority({authority_str});
                        check if resource($res), $res.starts_with({path_str});

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
        recipient: AuthorityId,
        expiration_seconds: u64,
    ) -> Result<Biscuit, BiscuitError> {
        let issuer = self.token_authority.authority_id().to_string();
        let recipient_str = recipient.to_string();
        let expiry_time = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64
            + expiration_seconds as i64;

        let expiring_token = biscuit!(
            r#"
            issuer({issuer});
            authority({recipient_str});
            role("temporary");
            capability("read");

            // Time-based expiration
            check if time($time), $time < {expiry_time};
        "#
        )
        .build(self.token_authority.root_keypair())?;

        Ok(expiring_token)
    }

    /// Create a token with delegation depth limit
    pub fn create_depth_limited_token(
        &self,
        recipient: AuthorityId,
        max_depth: u32,
    ) -> Result<Biscuit, BiscuitError> {
        let issuer = self.token_authority.authority_id().to_string();
        let recipient_str = recipient.to_string();
        let max_depth_i64 = max_depth as i64;

        let depth_limited_token = biscuit!(
            r#"
            issuer({issuer});
            authority({recipient_str});
            role("delegator");
            capability("read");
            capability("delegate");

            // Delegation depth limit
            delegation_max_depth({max_depth_i64});
            check if delegation_depth($depth), $depth <= {max_depth_i64};
        "#
        )
        .build(self.token_authority.root_keypair())?;

        Ok(depth_limited_token)
    }

    /// Get the root public key for token verification
    pub fn root_public_key(&self) -> PublicKey {
        self.token_authority.root_public_key()
    }

    /// Get an authority token manager
    pub fn get_authority_token(&self, authority_id: &AuthorityId) -> Option<&BiscuitTokenManager> {
        self.authority_tokens.get(authority_id)
    }

    /// Get a guardian token
    pub fn get_guardian_token(&self, authority_id: &AuthorityId) -> Option<&Biscuit> {
        self.guardian_tokens.get(authority_id)
    }

    /// Get a delegation chain by ID
    pub fn get_delegation_chain(&self, chain_id: &str) -> Option<&DelegatedTokenChain> {
        self.delegated_tokens
            .iter()
            .find(|chain| chain.chain_id == chain_id)
    }

    /// Create a token with minimal privileges (for testing privilege escalation prevention)
    pub fn create_minimal_token(&self, recipient: AuthorityId) -> Result<Biscuit, BiscuitError> {
        let issuer = self.token_authority.authority_id().to_string();
        let recipient_str = recipient.to_string();

        let minimal_token = biscuit!(
            r#"
            issuer({issuer});
            authority({recipient_str});
            role("read_only");

            // Very restricted capabilities
            capability("read");

            // Only allow reading from specific paths
            check if operation("read");
            check if resource($res), $res.starts_with("/storage/personal/read_only/");
        "#
        )
        .build(self.token_authority.root_keypair())?;

        Ok(minimal_token)
    }

    /// Create a compromised token scenario for security testing
    pub fn create_compromised_scenario(
        &self,
        recipient: AuthorityId,
    ) -> Result<Biscuit, BiscuitError> {
        let issuer = self.token_authority.authority_id().to_string();
        let recipient_str = recipient.to_string();

        // Create a token that might be used in privilege escalation attempts
        let suspicious_token = biscuit!(
            r#"
            issuer({issuer});
            authority({recipient_str});
            role("suspicious");
            capability("read");

            // This token will be used to test privilege escalation prevention
            check if operation("read");
            check if resource($res), $res.starts_with("/storage/public/");

            // Add suspicious facts that shouldn't grant additional privileges
            compromised_authority(true);
            attempted_privilege_escalation(true);
        "#
        )
        .build(self.token_authority.root_keypair())?;

        Ok(suspicious_token)
    }

    /// Get authority ID for this fixture
    pub fn authority_id(&self) -> AuthorityId {
        self.token_authority.authority_id()
    }
}

impl Default for BiscuitTestFixture {
    fn default() -> Self {
        Self::new()
    }
}

/// Convenience functions for creating common test scenarios
/// Create a basic multi-authority scenario with owner and guardians
pub fn create_multi_authority_scenario() -> Result<BiscuitTestFixture, BiscuitError> {
    let mut fixture = BiscuitTestFixture::new();

    // Add owner authority
    let owner_authority = authority(1);
    fixture.add_authority_token(owner_authority)?;

    // Add two guardians
    let guardian1 = authority(2);
    let guardian2 = authority(3);
    fixture.add_guardian_token(guardian1)?;
    fixture.add_guardian_token(guardian2)?;

    Ok(fixture)
}

/// Create a delegation chain scenario with progressive attenuation
pub fn create_delegation_scenario() -> Result<BiscuitTestFixture, BiscuitError> {
    let mut fixture = BiscuitTestFixture::new();

    let owner_authority = authority(4);
    fixture.add_authority_token(owner_authority)?;

    // Create a delegation chain with progressive restrictions using authority-based scopes
    let resource_scopes = vec![
        ResourceScope::Authority {
            authority_id: owner_authority,
            operation: AuthorityOp::UpdateTree,
        },
        ResourceScope::Storage {
            authority_id: owner_authority,
            path: "/documents/".to_string(), // Further restricted by attenuation blocks
        },
    ];

    fixture.create_delegation_chain("progressive_restriction", owner_authority, resource_scopes)?;

    Ok(fixture)
}

/// Create a recovery scenario with guardian tokens
pub fn create_recovery_scenario() -> Result<BiscuitTestFixture, BiscuitError> {
    let mut fixture = BiscuitTestFixture::new();

    // Add compromised authority (to be recovered)
    let compromised_authority = authority(5);
    fixture.add_authority_token(compromised_authority)?;

    // Add three guardians for 2-of-3 recovery
    for i in 0..3 {
        let guardian_authority = authority(6 + i);
        fixture.add_guardian_token(guardian_authority)?;
    }

    Ok(fixture)
}

/// Create a security testing scenario with various privilege levels
pub fn create_security_test_scenario() -> Result<BiscuitTestFixture, BiscuitError> {
    let mut fixture = BiscuitTestFixture::new();

    // Add authorities with different privilege levels
    let admin_authority = authority(20);
    let regular_authority = authority(21);
    let restricted_authority = authority(22);

    fixture.add_authority_token(admin_authority)?;
    fixture.add_authority_token(regular_authority)?;

    // Create restricted and compromised tokens for security testing
    let _minimal_token = fixture.create_minimal_token(restricted_authority)?;
    let _compromised_token = fixture.create_compromised_scenario(authority(23))?;

    Ok(fixture)
}

// Legacy compatibility aliases
#[deprecated(since = "0.2.0", note = "Use create_multi_authority_scenario instead")]
pub fn create_multi_device_scenario() -> Result<BiscuitTestFixture, BiscuitError> {
    create_multi_authority_scenario()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_fixture_creation() {
        let fixture = BiscuitTestFixture::new();
        assert!(fixture.authority_tokens.is_empty());
        assert!(fixture.guardian_tokens.is_empty());
        assert!(fixture.delegated_tokens.is_empty());
    }

    #[test]
    fn test_authority_token_creation() {
        let mut fixture = BiscuitTestFixture::new();
        let authority_id = authority(30);

        fixture.add_authority_token(authority_id).unwrap();
        assert!(fixture.authority_tokens.contains_key(&authority_id));
    }

    #[test]
    fn test_guardian_token_creation() {
        let mut fixture = BiscuitTestFixture::new();
        let authority_id = authority(31);

        fixture.add_guardian_token(authority_id).unwrap();
        assert!(fixture.guardian_tokens.contains_key(&authority_id));
    }

    #[test]
    fn test_multi_authority_scenario() {
        let fixture = create_multi_authority_scenario().unwrap();
        assert!(!fixture.authority_tokens.is_empty());
        assert!(!fixture.guardian_tokens.is_empty());
    }

    #[test]
    fn test_delegation_scenario() {
        let fixture = create_delegation_scenario().unwrap();
        assert!(!fixture.authority_tokens.is_empty());
        assert!(!fixture.delegated_tokens.is_empty());
    }

    #[test]
    fn test_recovery_scenario() {
        let fixture = create_recovery_scenario().unwrap();
        assert!(!fixture.authority_tokens.is_empty());
        assert!(!fixture.guardian_tokens.is_empty());
        assert_eq!(fixture.guardian_tokens.len(), 3);
    }

    #[test]
    fn test_security_test_scenario() {
        let fixture = create_security_test_scenario().unwrap();
        assert!(!fixture.authority_tokens.is_empty());
    }

    #[test]
    fn test_expiring_token_creation() {
        let fixture = BiscuitTestFixture::new();
        let authority_id = authority(32);

        let token = fixture.create_expiring_token(authority_id, 3600).unwrap();
        assert!(!token.to_vec().unwrap().is_empty());
    }

    #[test]
    fn test_depth_limited_token_creation() {
        let fixture = BiscuitTestFixture::new();
        let authority_id = authority(33);

        let token = fixture.create_depth_limited_token(authority_id, 3).unwrap();
        assert!(!token.to_vec().unwrap().is_empty());
    }
}
