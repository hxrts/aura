//! Coordinated Capability Management
//!
//! Unified capability manager handling Storage, Communication, and Relay permissions.
//! Provides grant, verify, delegate, and revoke operations with threshold signing support.
//!
//! Reference: docs/040_storage.md Section 2.1 "Permission" enum

use super::{
    CapabilityError, CapabilityId, CapabilityToken, CommunicationOperation, Permission,
    RelayOperation, Result, StorageOperation,
};
use crate::DeviceId;
use aura_crypto::Effects;
use ed25519_dalek::{SigningKey, VerifyingKey};
use std::collections::{BTreeMap, BTreeSet};

/// Capability grant request
#[derive(Debug, Clone)]
pub struct CapabilityGrant {
    pub device_id: DeviceId,
    pub permissions: Vec<Permission>,
    pub issued_at: u64,
    pub expires_at: Option<u64>,
    pub delegation_chain: Vec<CapabilityId>,
}

/// Coordinated capability manager
///
/// Handles capability grants, verification, delegation, and revocation
/// across all three permission types (Storage, Communication, Relay).
#[derive(Debug, Clone)]
pub struct CapabilityManager {
    /// Active capability tokens indexed by device
    tokens: BTreeMap<DeviceId, Vec<CapabilityToken>>,
    /// Revoked capability IDs
    revoked: BTreeSet<CapabilityId>,
    /// Delegation graph for cascading revocation
    delegation_graph: BTreeMap<CapabilityId, Vec<CapabilityId>>,
    /// Authority keys for verification
    authority_keys: BTreeMap<DeviceId, VerifyingKey>,
}

impl CapabilityManager {
    /// Create a new capability manager
    pub fn new() -> Self {
        Self {
            tokens: BTreeMap::new(),
            revoked: BTreeSet::new(),
            delegation_graph: BTreeMap::new(),
            authority_keys: BTreeMap::new(),
        }
    }

    /// Register an authority key for verification
    pub fn register_authority(&mut self, device_id: DeviceId, key: VerifyingKey) {
        self.authority_keys.insert(device_id, key);
    }

    /// Grant capability covering all three permission types
    ///
    /// This method can grant mixed permissions (e.g., storage + communication)
    /// in a single capability token for convenience.
    pub fn grant_capability(
        &mut self,
        grant: CapabilityGrant,
        signing_key: &SigningKey,
        effects: &Effects,
    ) -> Result<CapabilityToken> {
        // Validate permissions
        self.validate_permissions(&grant.permissions)?;

        // Create capability token directly with device_id
        let mut token = CapabilityToken::new(
            grant.device_id,
            grant.permissions.clone(),
            grant.delegation_chain.clone(),
            signing_key,
            effects,
        )
        .map_err(CapabilityError::CryptoError)?;

        // Set expiration if provided
        if let Some(expires_at) = grant.expires_at {
            token = token.with_expiration(expires_at);
        }

        // Store token
        self.tokens
            .entry(grant.device_id)
            .or_default()
            .push(token.clone());

        Ok(token)
    }

    /// Verify capability for storage access
    pub fn verify_storage(
        &self,
        device_id: &DeviceId,
        operation: StorageOperation,
        resource: &str,
        current_time: u64,
    ) -> Result<()> {
        let required = Permission::Storage {
            operation,
            resource: resource.to_string(),
        };

        self.verify_permission(device_id, &required, current_time)
    }

    /// Verify capability for communication
    pub fn verify_communication(
        &self,
        device_id: &DeviceId,
        operation: CommunicationOperation,
        relationship: &str,
        current_time: u64,
    ) -> Result<()> {
        let required = Permission::Communication {
            operation,
            relationship: relationship.to_string(),
        };

        self.verify_permission(device_id, &required, current_time)
    }

    /// Verify capability for relay
    pub fn verify_relay(
        &self,
        device_id: &DeviceId,
        operation: RelayOperation,
        trust_level: &str,
        current_time: u64,
    ) -> Result<()> {
        let required = Permission::Relay {
            operation,
            trust_level: trust_level.to_string(),
        };

        self.verify_permission(device_id, &required, current_time)
    }

    /// Unified permission verification
    fn verify_permission(
        &self,
        device_id: &DeviceId,
        required: &Permission,
        current_time: u64,
    ) -> Result<()> {
        let tokens = self
            .tokens
            .get(device_id)
            .ok_or(CapabilityError::AuthorizationError(
                "No capabilities found".to_string(),
            ))?;

        // Find a valid token with required permission
        for token in tokens {
            // Check expiration
            if token.is_expired(current_time) {
                continue;
            }

            // Check if token itself is revoked
            if self.revoked.contains(&token.capability_id()) {
                continue;
            }

            // Check if delegation chain is revoked
            if self.is_revoked(&token.delegation_chain)? {
                continue;
            }

            // Check permission match
            if self.has_permission(token, required) {
                return Ok(());
            }
        }

        Err(CapabilityError::AuthorizationError(
            "Insufficient permissions".to_string(),
        ))
    }

    /// Check if token has required permission
    fn has_permission(&self, token: &CapabilityToken, required: &Permission) -> bool {
        token
            .granted_permissions
            .iter()
            .any(|p| match (p, required) {
                (
                    Permission::Storage {
                        operation: op1,
                        resource: res1,
                    },
                    Permission::Storage {
                        operation: op2,
                        resource: res2,
                    },
                ) => op1 == op2 && (res1 == res2 || res1 == "*"),
                (
                    Permission::Communication {
                        operation: op1,
                        relationship: rel1,
                    },
                    Permission::Communication {
                        operation: op2,
                        relationship: rel2,
                    },
                ) => op1 == op2 && (rel1 == rel2 || rel1 == "*"),
                (
                    Permission::Relay {
                        operation: op1,
                        trust_level: trust1,
                    },
                    Permission::Relay {
                        operation: op2,
                        trust_level: trust2,
                    },
                ) => op1 == op2 && trust1 >= trust2,
                _ => false,
            })
    }

    /// Delegate capability with new restrictions
    pub fn delegate_capability(
        &mut self,
        parent_id: CapabilityId,
        child_device: DeviceId,
        restricted_permissions: Vec<Permission>,
        signing_key: &SigningKey,
        effects: &Effects,
    ) -> Result<CapabilityToken> {
        // Build delegation chain
        let delegation_chain = vec![parent_id.clone()];

        // Create grant with restricted permissions
        let grant = CapabilityGrant {
            device_id: child_device,
            permissions: restricted_permissions,
            issued_at: effects.now().unwrap_or(0),
            expires_at: None,
            delegation_chain: delegation_chain.clone(),
        };

        // Grant the capability
        let token = self.grant_capability(grant, signing_key, effects)?;

        // Record delegation relationship
        self.delegation_graph
            .entry(parent_id)
            .or_default()
            .push(token.capability_id());

        Ok(token)
    }

    /// Revoke capability with cascading to delegated capabilities
    pub fn revoke_capability(&mut self, capability_id: CapabilityId) -> Result<()> {
        // Mark as revoked
        self.revoked.insert(capability_id.clone());

        // Cascade to delegated capabilities
        if let Some(children) = self.delegation_graph.get(&capability_id) {
            for child_id in children.clone() {
                self.revoke_capability(child_id)?;
            }
        }

        Ok(())
    }

    /// Check if capability is revoked (including delegation chain)
    fn is_revoked(&self, delegation_chain: &[CapabilityId]) -> Result<bool> {
        for id in delegation_chain {
            if self.revoked.contains(id) {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// Validate permissions for consistency
    fn validate_permissions(&self, permissions: &[Permission]) -> Result<()> {
        if permissions.is_empty() {
            return Err(CapabilityError::AuthorizationError(
                "Empty permissions list".to_string(),
            ));
        }

        // Permissions are valid - no wildcards except for resource matching
        for perm in permissions {
            match perm {
                Permission::Storage { resource, .. } => {
                    if resource.is_empty() {
                        return Err(CapabilityError::AuthorizationError(
                            "Empty resource".to_string(),
                        ));
                    }
                }
                Permission::Communication { relationship, .. } => {
                    if relationship.is_empty() {
                        return Err(CapabilityError::AuthorizationError(
                            "Empty relationship".to_string(),
                        ));
                    }
                }
                Permission::Relay { trust_level, .. } => {
                    if trust_level.is_empty() {
                        return Err(CapabilityError::AuthorizationError(
                            "Empty trust level".to_string(),
                        ));
                    }
                }
            }
        }

        Ok(())
    }

    /// Get all capabilities for a device
    pub fn get_capabilities(&self, device_id: &DeviceId) -> Vec<CapabilityToken> {
        self.tokens.get(device_id).cloned().unwrap_or_default()
    }

    /// Remove expired capabilities
    pub fn cleanup_expired(&mut self, current_time: u64) {
        for tokens in self.tokens.values_mut() {
            tokens.retain(|t| !t.is_expired(current_time));
        }
    }
}

impl Default for CapabilityManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
#[allow(clippy::disallowed_methods, clippy::clone_on_copy)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn test_signing_key() -> SigningKey {
        SigningKey::from_bytes(&[1u8; 32])
    }

    fn test_effects() -> Effects {
        Effects::for_test("capability_manager_test")
    }

    fn test_device_id() -> DeviceId {
        DeviceId(Uuid::new_v4())
    }

    #[test]
    fn test_manager_creation() {
        let manager = CapabilityManager::new();
        assert_eq!(manager.tokens.len(), 0);
        assert_eq!(manager.revoked.len(), 0);
    }

    #[test]
    fn test_grant_storage_capability() {
        let mut manager = CapabilityManager::new();
        let device_id = test_device_id();
        let signing_key = test_signing_key();
        let effects = test_effects();

        let grant = CapabilityGrant {
            device_id: device_id.clone(),
            permissions: vec![Permission::Storage {
                operation: StorageOperation::Read,
                resource: "test/*".to_string(),
            }],
            issued_at: effects.now().unwrap_or(0),
            expires_at: None,
            delegation_chain: vec![],
        };

        let result = manager.grant_capability(grant, &signing_key, &effects);
        assert!(result.is_ok());

        let caps = manager.get_capabilities(&device_id);
        assert_eq!(caps.len(), 1);
    }

    #[test]
    fn test_grant_mixed_permissions() {
        let mut manager = CapabilityManager::new();
        let device_id = test_device_id();
        let signing_key = test_signing_key();
        let effects = test_effects();

        let grant = CapabilityGrant {
            device_id: device_id.clone(),
            permissions: vec![
                Permission::Storage {
                    operation: StorageOperation::Read,
                    resource: "data/*".to_string(),
                },
                Permission::Communication {
                    operation: CommunicationOperation::Send,
                    relationship: "friend".to_string(),
                },
            ],
            issued_at: effects.now().unwrap_or(0),
            expires_at: None,
            delegation_chain: vec![],
        };

        let result = manager.grant_capability(grant, &signing_key, &effects);
        assert!(result.is_ok());
    }

    #[test]
    fn test_verify_storage_permission() {
        let mut manager = CapabilityManager::new();
        let device_id = test_device_id();
        let signing_key = test_signing_key();
        let effects = test_effects();

        let grant = CapabilityGrant {
            device_id: device_id.clone(),
            permissions: vec![Permission::Storage {
                operation: StorageOperation::Read,
                resource: "*".to_string(),
            }],
            issued_at: effects.now().unwrap_or(0),
            expires_at: None,
            delegation_chain: vec![],
        };

        manager
            .grant_capability(grant, &signing_key, &effects)
            .unwrap();

        let result = manager.verify_storage(
            &device_id,
            StorageOperation::Read,
            "test/file",
            effects.now().unwrap_or(0),
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_verify_insufficient_permission() {
        let mut manager = CapabilityManager::new();
        let device_id = test_device_id();
        let signing_key = test_signing_key();
        let effects = test_effects();

        let grant = CapabilityGrant {
            device_id: device_id.clone(),
            permissions: vec![Permission::Storage {
                operation: StorageOperation::Read,
                resource: "public/*".to_string(),
            }],
            issued_at: effects.now().unwrap_or(0),
            expires_at: None,
            delegation_chain: vec![],
        };

        manager
            .grant_capability(grant, &signing_key, &effects)
            .unwrap();

        let result = manager.verify_storage(
            &device_id,
            StorageOperation::Write,
            "public/file",
            effects.now().unwrap_or(0),
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_capability_revocation() {
        let mut manager = CapabilityManager::new();
        let device_id = test_device_id();
        let signing_key = test_signing_key();
        let effects = test_effects();

        let grant = CapabilityGrant {
            device_id: device_id.clone(),
            permissions: vec![Permission::Storage {
                operation: StorageOperation::Read,
                resource: "*".to_string(),
            }],
            issued_at: effects.now().unwrap_or(0),
            expires_at: None,
            delegation_chain: vec![],
        };

        let token = manager
            .grant_capability(grant, &signing_key, &effects)
            .unwrap();
        let cap_id = token.capability_id();

        manager.revoke_capability(cap_id).unwrap();

        let result = manager.verify_storage(
            &device_id,
            StorageOperation::Read,
            "test",
            effects.now().unwrap_or(0),
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_delegation_chain() {
        let mut manager = CapabilityManager::new();
        let parent_device = test_device_id();
        let child_device = test_device_id();
        let signing_key = test_signing_key();
        let effects = test_effects();

        // Grant parent capability
        let parent_grant = CapabilityGrant {
            device_id: parent_device.clone(),
            permissions: vec![Permission::Storage {
                operation: StorageOperation::Read,
                resource: "*".to_string(),
            }],
            issued_at: effects.now().unwrap_or(0),
            expires_at: None,
            delegation_chain: vec![],
        };

        let parent_token = manager
            .grant_capability(parent_grant, &signing_key, &effects)
            .unwrap();

        // Delegate to child with restricted permissions
        let child_token = manager
            .delegate_capability(
                parent_token.capability_id(),
                child_device.clone(),
                vec![Permission::Storage {
                    operation: StorageOperation::Read,
                    resource: "public/*".to_string(),
                }],
                &signing_key,
                &effects,
            )
            .unwrap();

        assert!(child_token.delegation_chain.len() > 0);
    }

    #[test]
    fn test_cascading_revocation() {
        let mut manager = CapabilityManager::new();
        let parent_device = test_device_id();
        let child_device = test_device_id();
        let signing_key = test_signing_key();
        let effects = test_effects();

        // Setup parent and child capabilities
        let parent_grant = CapabilityGrant {
            device_id: parent_device,
            permissions: vec![Permission::Storage {
                operation: StorageOperation::Read,
                resource: "*".to_string(),
            }],
            issued_at: effects.now().unwrap_or(0),
            expires_at: None,
            delegation_chain: vec![],
        };

        let parent_token = manager
            .grant_capability(parent_grant, &signing_key, &effects)
            .unwrap();

        manager
            .delegate_capability(
                parent_token.capability_id(),
                child_device.clone(),
                vec![Permission::Storage {
                    operation: StorageOperation::Read,
                    resource: "data/*".to_string(),
                }],
                &signing_key,
                &effects,
            )
            .unwrap();

        // Revoke parent
        manager
            .revoke_capability(parent_token.capability_id())
            .unwrap();

        // Child should also be revoked
        let result = manager.verify_storage(
            &child_device,
            StorageOperation::Read,
            "data/file",
            effects.now().unwrap_or(0),
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_cleanup_expired() {
        let mut manager = CapabilityManager::new();
        let device_id = test_device_id();
        let signing_key = test_signing_key();
        let effects = test_effects();

        let grant = CapabilityGrant {
            device_id: device_id.clone(),
            permissions: vec![Permission::Storage {
                operation: StorageOperation::Read,
                resource: "*".to_string(),
            }],
            issued_at: effects.now().unwrap_or(0),
            expires_at: Some(effects.now().unwrap_or(0) + 100),
            delegation_chain: vec![],
        };

        manager
            .grant_capability(grant, &signing_key, &effects)
            .unwrap();

        assert_eq!(manager.get_capabilities(&device_id).len(), 1);

        manager.cleanup_expired(effects.now().unwrap_or(0) + 200);

        assert_eq!(manager.get_capabilities(&device_id).len(), 0);
    }
}
