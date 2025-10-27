//! Capability Types and Core Definitions
//!
//! Defines the core capability system for storage access control:
//! - **Capability Tokens**: Credentials with permissions and delegation chains
//! - **Permissions**: Storage operations (read, write, delete, list)
//! - **Resource Scopes**: What resources are accessible
//! - **Delegation Chains**: Cryptographic proof of capability delegation
//!
//! Reference: docs/040_storage.md Section 3

use crate::manifest::{
    CapabilityId, DeviceId, Permission, ResourceScope, StorageOperation, ThresholdSignature,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// Capability token - credential for storage access
///
/// A capability token grants specific permissions to a device.
/// Tokens can be delegated to other devices via delegation chains,
/// and include threshold signatures for authentication.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CapabilityToken {
    /// Device that holds this capability
    pub authenticated_device: DeviceId,

    /// Permissions granted by this token
    pub granted_permissions: Vec<Permission>,

    /// Chain of delegations (proof of authority)
    pub delegation_chain: Vec<CapabilityId>,

    /// Threshold signature authenticating this token
    pub signature: ThresholdSignature,

    /// Timestamp when token was issued
    pub issued_at: u64,

    /// Optional expiration timestamp
    pub expires_at: Option<u64>,
}

impl CapabilityToken {
    /// Create a new capability token
    pub fn new(
        authenticated_device: DeviceId,
        granted_permissions: Vec<Permission>,
        signature: ThresholdSignature,
        issued_at: u64,
    ) -> Self {
        Self {
            authenticated_device,
            granted_permissions,
            delegation_chain: vec![],
            signature,
            issued_at,
            expires_at: None,
        }
    }

    /// Add delegation chain to token
    pub fn with_delegation_chain(mut self, chain: Vec<CapabilityId>) -> Self {
        self.delegation_chain = chain;
        self
    }

    /// Set expiration time
    pub fn with_expiration(mut self, expires_at: u64) -> Self {
        self.expires_at = Some(expires_at);
        self
    }

    /// Check if token is expired at given time
    pub fn is_expired(&self, current_time: u64) -> bool {
        if let Some(expires_at) = self.expires_at {
            current_time > expires_at
        } else {
            false
        }
    }

    /// Check if token has specific operation permission
    pub fn has_operation(&self, operation: StorageOperation) -> bool {
        self.granted_permissions
            .iter()
            .any(|p| p.operation == operation)
    }

    /// Get all resource scopes for a given operation
    pub fn scopes_for_operation(&self, operation: StorageOperation) -> Vec<ResourceScope> {
        self.granted_permissions
            .iter()
            .filter(|p| p.operation == operation)
            .map(|p| p.resource.clone())
            .collect()
    }
}

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
        signature: ThresholdSignature,
        issued_at: u64,
    ) -> Result<CapabilityToken, CapabilityError> {
        let permission = Permission {
            operation,
            resource,
            grant_time: issued_at,
            expiry: None,
        };
        let token = CapabilityToken::new(device_id.clone(), vec![permission], signature, issued_at);

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
#[derive(Debug, Clone)]
pub enum CapabilityError {
    /// Token not found
    TokenNotFound,

    /// Token is expired
    TokenExpired,

    /// Token is revoked
    TokenRevoked,

    /// Permission denied
    PermissionDenied,

    /// Invalid signature
    InvalidSignature,

    /// Capability validation failed
    ValidationFailed(String),
}

impl std::fmt::Display for CapabilityError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CapabilityError::TokenNotFound => write!(f, "Token not found"),
            CapabilityError::TokenExpired => write!(f, "Token expired"),
            CapabilityError::TokenRevoked => write!(f, "Token revoked"),
            CapabilityError::PermissionDenied => write!(f, "Permission denied"),
            CapabilityError::InvalidSignature => write!(f, "Invalid signature"),
            CapabilityError::ValidationFailed(reason) => write!(f, "Validation failed: {}", reason),
        }
    }
}

impl std::error::Error for CapabilityError {}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_crypto::Effects;
    use aura_types::DeviceIdExt;

    fn create_test_token() -> CapabilityToken {
        CapabilityToken {
            authenticated_device: aura_types::DeviceId::new_with_effects(&Effects::test()),
            granted_permissions: vec![Permission {
                operation: StorageOperation::Read,
                resource: ResourceScope::Public,
                grant_time: 1000,
                expiry: None,
            }],
            delegation_chain: vec![],
            signature: ThresholdSignature {
                threshold: 2,
                signature_shares: vec![],
            },
            issued_at: 1000,
            expires_at: Some(2000),
        }
    }

    #[test]
    fn test_token_creation() {
        let token = create_test_token();
        assert_eq!(token.issued_at, 1000);
        assert_eq!(token.expires_at, Some(2000));
    }

    #[test]
    fn test_token_not_expired() {
        let token = create_test_token();
        assert!(!token.is_expired(1500));
    }

    #[test]
    fn test_token_expired() {
        let token = create_test_token();
        assert!(token.is_expired(2500));
    }

    #[test]
    fn test_token_no_expiration() {
        let mut token = create_test_token();
        token.expires_at = None;
        assert!(!token.is_expired(999999));
    }

    #[test]
    fn test_token_has_operation() {
        let token = create_test_token();
        assert!(token.has_operation(StorageOperation::Read));
        assert!(!token.has_operation(StorageOperation::Write));
    }

    #[test]
    fn test_capability_manager_grant() {
        let mut manager = CapabilityManager::new();
        let device_id = aura_types::DeviceId::new_with_effects(&Effects::test());

        let token = manager
            .grant_capability(
                device_id.clone(),
                StorageOperation::Read,
                ResourceScope::Public,
                ThresholdSignature {
                    threshold: 2,
                    signature_shares: vec![],
                },
                1000,
            )
            .unwrap();

        assert_eq!(token.authenticated_device, device_id);
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
        let device_id = aura_types::DeviceId::new_with_effects(&Effects::test());

        manager
            .grant_capability(
                device_id.clone(),
                StorageOperation::Read,
                ResourceScope::Public,
                ThresholdSignature {
                    threshold: 2,
                    signature_shares: vec![],
                },
                1000,
            )
            .unwrap();

        manager
            .grant_capability(
                device_id.clone(),
                StorageOperation::Write,
                ResourceScope::Public,
                ThresholdSignature {
                    threshold: 2,
                    signature_shares: vec![],
                },
                1000,
            )
            .unwrap();

        let capabilities = manager.list_device_capabilities(&device_id);
        assert_eq!(capabilities.len(), 2);
    }
}
