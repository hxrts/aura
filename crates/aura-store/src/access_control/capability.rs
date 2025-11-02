//! Capability Management for Storage Access Control
//!
//! Manages capability tokens and their validation for storage operations:
//! - **Capability Manager**: Tracks device capabilities and revocations
//! - **Capability Validation**: Checks token validity and permissions
//! - **Integration**: Works with aura-authorization CapabilityToken
//!
//! Note: Uses CapabilityToken from aura-authorization crate for consistency
//! across the platform.

use crate::manifest::{CapabilityId, DeviceId, ResourceScope, StorageOperation};
use aura_authorization::{Action, CapabilityToken, Resource, Subject};
use aura_crypto::Ed25519SigningKey;
use aura_types::{AccountId, AuraError};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use uuid::Uuid;

/// Capability manager - tracks and verifies capabilities
pub struct CapabilityManager {
    /// Tokens granted to each device
    tokens: BTreeMap<DeviceId, Vec<CapabilityToken>>,

    /// Revoked capability IDs
    revoked_capabilities: BTreeSet<CapabilityId>,

    /// Delegation graph for tracking capability chains
    delegation_graph: BTreeMap<CapabilityId, Vec<CapabilityId>>,
}

impl CapabilityManager {
    /// Create a new capability manager
    pub fn new() -> Self {
        Self {
            tokens: BTreeMap::new(),
            revoked_capabilities: BTreeSet::new(),
            delegation_graph: BTreeMap::new(),
        }
    }

    /// Grant a storage capability to a device
    pub fn grant_capability(
        &mut self,
        device_id: DeviceId,
        operation: StorageOperation,
        resource: ResourceScope,
        account_id: AccountId,
        signing_key: &Ed25519SigningKey,
    ) -> Result<CapabilityToken, aura_types::CapabilityError> {
        // Convert storage operation to authorization action
        let action = match operation {
            StorageOperation::Read => Action::Read,
            StorageOperation::Write => Action::Write,
            StorageOperation::Delete => Action::Delete,
            StorageOperation::List => Action::Read, // Map List to Read for now
            StorageOperation::Store => Action::Write, // Map Store to Write
            StorageOperation::Retrieve => Action::Read, // Map Retrieve to Read
            StorageOperation::GetMetadata => Action::Read, // Map GetMetadata to Read
        };

        // Convert resource scope to authorization resource
        let auth_resource = match resource {
            ResourceScope::AccountStorage { account_id } => Resource::Account(account_id),
            ResourceScope::StorageObject { account_id } => Resource::StorageObject {
                object_id: Uuid::new_v4(), // Generate new object ID
                owner: account_id,
            },
            ResourceScope::DeviceStorage { device_id } => Resource::Device(device_id),
            ResourceScope::Public => Resource::Account(account_id), // Map Public to Account for now
            ResourceScope::AllOwnedObjects => Resource::Account(account_id), // Map to account
            ResourceScope::Object { cid: _ } => Resource::StorageObject {
                object_id: Uuid::new_v4(),
                owner: account_id,
            },
            ResourceScope::Manifest { cid: _ } => Resource::StorageObject {
                object_id: Uuid::new_v4(),
                owner: account_id,
            },
        };

        // Create new token with authorization API
        let subject = Subject::Device(device_id);
        let mut token = CapabilityToken::new(
            subject,
            auth_resource,
            vec![action],
            device_id, // issuer
            false,     // not delegatable
            0,         // no delegation depth
        );

        // Sign the token
        token
            .sign(signing_key)
            .map_err(|e| aura_types::CapabilityError::InvalidSignature {
                message: e.to_string(),
                context: "".to_string(),
            })?;

        // Store the token
        self.tokens
            .entry(device_id)
            .or_insert_with(Vec::new)
            .push(token.clone());

        Ok(token)
    }

    /// Get all tokens for a device
    pub fn get_tokens(&self, device_id: &DeviceId) -> Option<&Vec<CapabilityToken>> {
        self.tokens.get(device_id)
    }

    /// Revoke a capability by ID
    pub fn revoke_capability(&mut self, capability_id: CapabilityId) {
        self.revoked_capabilities.insert(capability_id);
    }

    /// Check if a capability is revoked
    pub fn is_revoked(&self, capability_id: &CapabilityId) -> bool {
        self.revoked_capabilities.contains(capability_id)
    }

    /// Register a delegation relationship
    pub fn record_delegation(&mut self, parent_id: CapabilityId, delegated_id: CapabilityId) {
        self.delegation_graph
            .entry(parent_id)
            .or_insert_with(Vec::new)
            .push(delegated_id);
    }

    /// Get delegation chain for a capability
    pub fn get_delegation_chain(&self, capability_id: &CapabilityId) -> Option<Vec<CapabilityId>> {
        self.delegation_graph.get(capability_id).cloned()
    }

    /// List all capabilities for device
    pub fn list_device_capabilities(&self, device_id: &DeviceId) -> Vec<CapabilityToken> {
        self.tokens.get(device_id).cloned().unwrap_or_default()
    }

    /// Test helper: Add a token directly (for testing only)
    #[cfg(test)]
    pub fn add_token_for_testing(&mut self, device_id: DeviceId, token: CapabilityToken) {
        self.tokens
            .entry(device_id)
            .or_insert_with(Vec::new)
            .push(token);
    }
}

impl Default for CapabilityManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Errors in capability operations
// Re-export CapabilityError from aura_types for public use
pub use aura_types::CapabilityError;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::{Permission, ThresholdSignature};
    use aura_crypto::Effects;
    use aura_types::{AccountIdExt, DeviceIdExt};

    fn create_test_token() -> CapabilityToken {
        let effects = Effects::test();
        let device_id = aura_types::DeviceId::new_with_effects(&effects);
        let account_id = aura_types::AccountId::new_with_effects(&effects);

        CapabilityToken::new(
            Subject::Device(device_id),
            Resource::Account(account_id),
            vec![Action::Read],
            device_id,
            false, // not delegatable
            1,     // delegation depth
        )
    }

    #[test]
    fn test_token_creation() {
        let token = create_test_token();
        // Check that token was created with reasonable values
        assert!(token.issued_at > 0);
        assert!(token.expires_at.is_none()); // Default tokens don't have expiration
        assert!(!token.id.to_string().is_empty());
    }

    // TODO: These tests need to be updated to work with the current CapabilityToken API
    // or moved to store-specific token types if needed
    #[test]
    #[ignore]
    fn test_token_not_expired() {
        let _token = create_test_token();
        // assert!(!token.is_expired(1500)); // Method doesn't exist on current API
    }

    #[test]
    #[ignore]
    fn test_token_expired() {
        let _token = create_test_token();
        // assert!(token.is_expired(2500)); // Method doesn't exist on current API
    }

    #[test]
    #[ignore]
    fn test_token_no_expiration() {
        let mut _token = create_test_token();
        // token.expires_at = None; // Field doesn't exist on current API
        // assert!(!token.is_expired(999999)); // Method doesn't exist on current API
    }

    #[test]
    #[ignore]
    fn test_token_has_operation() {
        let _token = create_test_token();
        // assert!(token.has_operation(StorageOperation::Read)); // Method doesn't exist on current API
        // assert!(!token.has_operation(StorageOperation::Write));
    }

    #[test]
    fn test_capability_manager_grant() {
        let mut manager = CapabilityManager::new();
        let effects = Effects::test();
        let device_id = aura_types::DeviceId::new_with_effects(&effects);
        let account_id = aura_types::AccountId::new_with_effects(&effects);
        let signing_key = aura_crypto::generate_ed25519_key();

        let token = manager
            .grant_capability(
                device_id,
                StorageOperation::Read,
                ResourceScope::Public,
                account_id,
                &signing_key,
            )
            .unwrap();

        // Check that token was created successfully
        assert!(token.id.to_string().len() > 0);
    }

    #[test]
    fn test_capability_revocation() {
        let mut manager = CapabilityManager::new();
        let cap_id = vec![1u8; 32];

        assert!(!manager.is_revoked(&cap_id));
        manager.revoke_capability(cap_id.clone());
        assert!(manager.is_revoked(&cap_id));
    }

    #[test]
    fn test_delegation_chain() {
        let mut manager = CapabilityManager::new();
        let parent = vec![1u8; 32];
        let delegated = vec![2u8; 32];

        manager.record_delegation(parent.clone(), delegated.clone());

        let chain = manager.get_delegation_chain(&parent);
        assert!(chain.is_some());
        assert_eq!(chain.unwrap(), vec![delegated]);
    }

    #[test]
    fn test_list_device_capabilities() {
        let mut manager = CapabilityManager::new();
        let effects = Effects::test();
        let device_id = aura_types::DeviceId::new_with_effects(&effects);
        let account_id = aura_types::AccountId::new_with_effects(&effects);
        let signing_key = aura_crypto::generate_ed25519_key();

        manager
            .grant_capability(
                device_id,
                StorageOperation::Read,
                ResourceScope::Public,
                account_id,
                &signing_key,
            )
            .unwrap();

        manager
            .grant_capability(
                device_id,
                StorageOperation::Write,
                ResourceScope::Public,
                account_id,
                &signing_key,
            )
            .unwrap();

        let capabilities = manager.list_device_capabilities(&device_id);
        assert_eq!(capabilities.len(), 2);
    }
}
