//! Capability-Based Access Control for Storage
//!
//! Implements verification and granting of storage capabilities
//! with precise permission checking and threshold signatures.
//!
//! Reference: docs/040_storage.md Section 3

use crate::manifest::{
    AccessControl, CapabilityId, DeviceId, Permission, ResourceScope, StorageOperation,
    ThresholdSignature,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CapabilityToken {
    pub authenticated_device: DeviceId,
    pub granted_permissions: Vec<Permission>,
    pub delegation_chain: Vec<CapabilityId>,
    pub signature: ThresholdSignature,
    pub issued_at: u64,
    pub expires_at: Option<u64>,
}

impl CapabilityToken {
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

    pub fn with_delegation_chain(mut self, chain: Vec<CapabilityId>) -> Self {
        self.delegation_chain = chain;
        self
    }

    pub fn with_expiration(mut self, expires_at: u64) -> Self {
        self.expires_at = Some(expires_at);
        self
    }

    pub fn is_expired(&self, current_time: u64) -> bool {
        if let Some(expires_at) = self.expires_at {
            current_time > expires_at
        } else {
            false
        }
    }
}

pub struct CapabilityManager {
    tokens: BTreeMap<DeviceId, Vec<CapabilityToken>>,
    revoked_capabilities: BTreeSet<CapabilityId>,
    delegation_graph: BTreeMap<CapabilityId, Vec<CapabilityId>>,
}

impl CapabilityManager {
    pub fn new() -> Self {
        Self {
            tokens: BTreeMap::new(),
            revoked_capabilities: BTreeSet::new(),
            delegation_graph: BTreeMap::new(),
        }
    }

    pub fn grant_storage_capability(
        &mut self,
        device_id: DeviceId,
        operation: StorageOperation,
        resource: ResourceScope,
        signature: ThresholdSignature,
        issued_at: u64,
    ) -> Result<CapabilityToken, CapabilityError> {
        let permission = Permission::Storage {
            operation,
            resource,
        };

        let token = CapabilityToken::new(device_id.clone(), vec![permission], signature, issued_at);

        self.tokens
            .entry(device_id)
            .or_insert_with(Vec::new)
            .push(token.clone());

        Ok(token)
    }

    pub fn verify_storage_permissions(
        &self,
        device_id: &DeviceId,
        required_access: &AccessControl,
        current_time: u64,
    ) -> Result<(), CapabilityError> {
        let device_tokens = self
            .tokens
            .get(device_id)
            .ok_or(CapabilityError::NoCapabilities)?;

        let active_tokens: Vec<_> = device_tokens
            .iter()
            .filter(|t| !t.is_expired(current_time))
            .collect();

        if active_tokens.is_empty() {
            return Err(CapabilityError::ExpiredCapabilities);
        }

        match required_access {
            AccessControl::CapabilityBased {
                required_permissions,
                delegation_chain,
            } => {
                for token in &active_tokens {
                    if self.is_revoked(&token.delegation_chain)? {
                        continue;
                    }

                    if !delegation_chain.is_empty()
                        && !self.verify_delegation_chain(delegation_chain)?
                    {
                        continue;
                    }

                    if self.has_required_permissions(token, required_permissions) {
                        return Ok(());
                    }
                }

                Err(CapabilityError::InsufficientPermissions)
            }
        }
    }

    fn has_required_permissions(
        &self,
        token: &CapabilityToken,
        required_permissions: &[Permission],
    ) -> bool {
        for required in required_permissions {
            if !self.permission_matches(&token.granted_permissions, required) {
                return false;
            }
        }
        true
    }

    fn permission_matches(&self, granted: &[Permission], required: &Permission) -> bool {
        for grant in granted {
            match (grant, required) {
                (
                    Permission::Storage {
                        operation: granted_op,
                        resource: granted_res,
                    },
                    Permission::Storage {
                        operation: required_op,
                        resource: required_res,
                    },
                ) => {
                    if self.operation_satisfies(granted_op, required_op)
                        && self.resource_satisfies(granted_res, required_res)
                    {
                        return true;
                    }
                }
                _ => {}
            }
        }
        false
    }

    fn operation_satisfies(&self, granted: &StorageOperation, required: &StorageOperation) -> bool {
        granted == required
    }

    fn resource_satisfies(&self, granted: &ResourceScope, required: &ResourceScope) -> bool {
        match (granted, required) {
            (ResourceScope::AllOwnedObjects, _) => true,
            (
                ResourceScope::Object { cid: granted_cid },
                ResourceScope::Object { cid: required_cid },
            ) => granted_cid == required_cid,
            (
                ResourceScope::Manifest { cid: granted_cid },
                ResourceScope::Manifest { cid: required_cid },
            ) => granted_cid == required_cid,
            _ => false,
        }
    }

    pub fn revoke_capability(
        &mut self,
        capability_id: CapabilityId,
    ) -> Result<(), CapabilityError> {
        self.revoked_capabilities.insert(capability_id.clone());

        if let Some(delegated) = self.delegation_graph.remove(&capability_id) {
            for child_id in delegated {
                self.revoke_capability(child_id)?;
            }
        }

        Ok(())
    }

    fn is_revoked(&self, delegation_chain: &[CapabilityId]) -> Result<bool, CapabilityError> {
        for cap_id in delegation_chain {
            if self.revoked_capabilities.contains(cap_id) {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn verify_delegation_chain(&self, chain: &[CapabilityId]) -> Result<bool, CapabilityError> {
        if chain.is_empty() {
            return Ok(true);
        }

        for (i, cap_id) in chain.iter().enumerate() {
            if self.revoked_capabilities.contains(cap_id) {
                return Ok(false);
            }

            if i > 0 {
                let parent = &chain[i - 1];
                if let Some(children) = self.delegation_graph.get(parent) {
                    if !children.contains(cap_id) {
                        return Ok(false);
                    }
                } else {
                    return Ok(false);
                }
            }
        }

        Ok(true)
    }

    pub fn delegate_capability(
        &mut self,
        parent_capability: CapabilityId,
        child_capability: CapabilityId,
    ) -> Result<(), CapabilityError> {
        if self.revoked_capabilities.contains(&parent_capability) {
            return Err(CapabilityError::ParentRevoked);
        }

        self.delegation_graph
            .entry(parent_capability)
            .or_insert_with(Vec::new)
            .push(child_capability);

        Ok(())
    }

    pub fn get_device_capabilities(&self, device_id: &DeviceId) -> Option<&Vec<CapabilityToken>> {
        self.tokens.get(device_id)
    }

    pub fn cleanup_expired_tokens(&mut self, current_time: u64) {
        for tokens in self.tokens.values_mut() {
            tokens.retain(|t| !t.is_expired(current_time));
        }

        self.tokens.retain(|_, tokens| !tokens.is_empty());
    }
}

#[derive(Debug, Clone)]
pub enum CapabilityError {
    NoCapabilities,
    ExpiredCapabilities,
    InsufficientPermissions,
    InvalidDelegation,
    ParentRevoked,
    VerificationFailed(String),
}

impl std::fmt::Display for CapabilityError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoCapabilities => write!(f, "No capabilities found for device"),
            Self::ExpiredCapabilities => write!(f, "All capabilities have expired"),
            Self::InsufficientPermissions => write!(f, "Insufficient permissions"),
            Self::InvalidDelegation => write!(f, "Invalid delegation chain"),
            Self::ParentRevoked => write!(f, "Parent capability has been revoked"),
            Self::VerificationFailed(msg) => write!(f, "Verification failed: {}", msg),
        }
    }
}

impl std::error::Error for CapabilityError {}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_signature() -> ThresholdSignature {
        ThresholdSignature::placeholder()
    }

    fn create_test_device_id() -> DeviceId {
        vec![1u8; 32]
    }

    #[test]
    fn test_capability_token_creation() {
        let device_id = create_test_device_id();
        let permission = Permission::Storage {
            operation: StorageOperation::Read,
            resource: ResourceScope::AllOwnedObjects,
        };
        let sig = create_test_signature();

        let token = CapabilityToken::new(device_id.clone(), vec![permission], sig, 1000);

        assert_eq!(token.authenticated_device, device_id);
        assert_eq!(token.granted_permissions.len(), 1);
        assert!(!token.is_expired(2000));
    }

    #[test]
    fn test_capability_token_expiration() {
        let token = CapabilityToken::new(
            create_test_device_id(),
            vec![],
            create_test_signature(),
            1000,
        )
        .with_expiration(2000);

        assert!(!token.is_expired(1500));
        assert!(token.is_expired(2001));
    }

    #[test]
    fn test_grant_storage_capability() {
        let mut manager = CapabilityManager::new();
        let device_id = create_test_device_id();

        let result = manager.grant_storage_capability(
            device_id.clone(),
            StorageOperation::Read,
            ResourceScope::AllOwnedObjects,
            create_test_signature(),
            1000,
        );

        assert!(result.is_ok());

        let tokens = manager.get_device_capabilities(&device_id).unwrap();
        assert_eq!(tokens.len(), 1);
    }

    #[test]
    fn test_verify_storage_permissions_success() {
        let mut manager = CapabilityManager::new();
        let device_id = create_test_device_id();

        manager
            .grant_storage_capability(
                device_id.clone(),
                StorageOperation::Read,
                ResourceScope::AllOwnedObjects,
                create_test_signature(),
                1000,
            )
            .unwrap();

        let access = AccessControl::CapabilityBased {
            required_permissions: vec![Permission::Storage {
                operation: StorageOperation::Read,
                resource: ResourceScope::AllOwnedObjects,
            }],
            delegation_chain: vec![],
        };

        let result = manager.verify_storage_permissions(&device_id, &access, 1500);
        assert!(result.is_ok());
    }

    #[test]
    fn test_verify_storage_permissions_insufficient() {
        let mut manager = CapabilityManager::new();
        let device_id = create_test_device_id();

        manager
            .grant_storage_capability(
                device_id.clone(),
                StorageOperation::Read,
                ResourceScope::AllOwnedObjects,
                create_test_signature(),
                1000,
            )
            .unwrap();

        let access = AccessControl::CapabilityBased {
            required_permissions: vec![Permission::Storage {
                operation: StorageOperation::Write,
                resource: ResourceScope::AllOwnedObjects,
            }],
            delegation_chain: vec![],
        };

        let result = manager.verify_storage_permissions(&device_id, &access, 1500);
        assert!(matches!(
            result,
            Err(CapabilityError::InsufficientPermissions)
        ));
    }

    #[test]
    fn test_revoke_capability() {
        let mut manager = CapabilityManager::new();
        let cap_id = vec![1u8; 32];

        manager.revoke_capability(cap_id.clone()).unwrap();
        assert!(manager.revoked_capabilities.contains(&cap_id));
    }

    #[test]
    fn test_delegation_chain() {
        let mut manager = CapabilityManager::new();
        let parent_cap = vec![1u8; 32];
        let child_cap = vec![2u8; 32];

        manager
            .delegate_capability(parent_cap.clone(), child_cap.clone())
            .unwrap();

        let chain = vec![parent_cap.clone(), child_cap.clone()];
        assert!(manager.verify_delegation_chain(&chain).unwrap());
    }

    #[test]
    fn test_delegation_revocation_cascades() {
        let mut manager = CapabilityManager::new();
        let parent_cap = vec![1u8; 32];
        let child_cap = vec![2u8; 32];

        manager
            .delegate_capability(parent_cap.clone(), child_cap.clone())
            .unwrap();
        manager.revoke_capability(parent_cap.clone()).unwrap();

        assert!(manager.revoked_capabilities.contains(&parent_cap));
        assert!(manager.revoked_capabilities.contains(&child_cap));
    }

    #[test]
    fn test_cleanup_expired_tokens() {
        let mut manager = CapabilityManager::new();
        let device_id = create_test_device_id();

        let token1 = CapabilityToken::new(device_id.clone(), vec![], create_test_signature(), 1000)
            .with_expiration(2000);

        let token2 = CapabilityToken::new(device_id.clone(), vec![], create_test_signature(), 1000)
            .with_expiration(3000);

        manager
            .tokens
            .insert(device_id.clone(), vec![token1, token2]);

        manager.cleanup_expired_tokens(2500);

        let tokens = manager.get_device_capabilities(&device_id).unwrap();
        assert_eq!(tokens.len(), 1);
        assert_eq!(tokens[0].expires_at, Some(3000));
    }

    #[test]
    fn test_all_owned_objects_permission() {
        let mut manager = CapabilityManager::new();
        let device_id = create_test_device_id();

        manager
            .grant_storage_capability(
                device_id.clone(),
                StorageOperation::Read,
                ResourceScope::AllOwnedObjects,
                create_test_signature(),
                1000,
            )
            .unwrap();

        let specific_access = AccessControl::CapabilityBased {
            required_permissions: vec![Permission::Storage {
                operation: StorageOperation::Read,
                resource: ResourceScope::Object {
                    cid: vec![42u8; 32],
                },
            }],
            delegation_chain: vec![],
        };

        let result = manager.verify_storage_permissions(&device_id, &specific_access, 1500);
        assert!(result.is_ok());
    }

    #[test]
    fn test_specific_object_permission() {
        let mut manager = CapabilityManager::new();
        let device_id = create_test_device_id();
        let allowed_cid = vec![1u8; 32];
        let other_cid = vec![2u8; 32];

        manager
            .grant_storage_capability(
                device_id.clone(),
                StorageOperation::Read,
                ResourceScope::Object {
                    cid: allowed_cid.clone(),
                },
                create_test_signature(),
                1000,
            )
            .unwrap();

        let allowed_access = AccessControl::CapabilityBased {
            required_permissions: vec![Permission::Storage {
                operation: StorageOperation::Read,
                resource: ResourceScope::Object {
                    cid: allowed_cid.clone(),
                },
            }],
            delegation_chain: vec![],
        };

        assert!(manager
            .verify_storage_permissions(&device_id, &allowed_access, 1500)
            .is_ok());

        let denied_access = AccessControl::CapabilityBased {
            required_permissions: vec![Permission::Storage {
                operation: StorageOperation::Read,
                resource: ResourceScope::Object { cid: other_cid },
            }],
            delegation_chain: vec![],
        };

        assert!(manager
            .verify_storage_permissions(&device_id, &denied_access, 1500)
            .is_err());
    }
}
