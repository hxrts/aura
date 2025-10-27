//! Capability Verification Logic
//!
//! Implements verification of storage capabilities and permissions:
//! - **Token Validation**: Check token authenticity and expiration
//! - **Permission Checking**: Verify specific operation permissions
//! - **Delegation Verification**: Validate capability delegation chains
//! - **Revocation Checking**: Ensure capabilities haven't been revoked
//!
//! Separated from capability types to keep verification logic independent
//! of capability structure.

use crate::access_control::capability::{CapabilityError, CapabilityManager, CapabilityToken};
use crate::error::{Result, StoreError, StoreErrorBuilder};
use crate::manifest::{ResourceScope, StorageOperation};
use aura_types::{DeviceId, DeviceIdExt};

/// Capability checker - verifies storage access permissions
pub struct CapabilityChecker {
    manager: CapabilityManager,
}

impl CapabilityChecker {
    /// Create a new capability checker
    pub fn new(manager: CapabilityManager) -> Self {
        Self { manager }
    }

    /// Verify a device can perform an operation on a resource
    pub fn verify_access(
        &self,
        device_id: &DeviceId,
        operation: StorageOperation,
        resource: &ResourceScope,
        current_time: u64,
    ) -> Result<()> {
        let tokens = self
            .manager
            .get_tokens(device_id)
            .ok_or_else(|| StoreErrorBuilder::access_denied("No capabilities granted to device"))?;

        // Check if any token grants the required permission
        for token in tokens {
            if self
                .token_grants_access(token, operation, resource, current_time)
                .is_ok()
            {
                return Ok(());
            }
        }

        Err(StoreErrorBuilder::access_denied(format!(
            "No token grants {} access to resource",
            operation as u32
        )))
    }

    /// Check if a specific token grants access
    fn token_grants_access(
        &self,
        token: &CapabilityToken,
        operation: StorageOperation,
        resource: &ResourceScope,
        current_time: u64,
    ) -> Result<()> {
        // Check expiration
        if token.is_expired(current_time) {
            return Err(StoreErrorBuilder::capability_expired(
                token.expires_at.unwrap_or(0),
            ));
        }

        // Check revocation
        for cap_id in &token.delegation_chain {
            if self.manager.is_revoked(cap_id) {
                return Err(StoreErrorBuilder::capability_revoked(hex::encode(cap_id)));
            }
        }

        // Check permissions
        let has_permission = token
            .granted_permissions
            .iter()
            .any(|p| p.operation == operation);
        // TODO: Add resource matching when Permission struct includes resource field

        if has_permission {
            Ok(())
        } else {
            Err(StoreErrorBuilder::insufficient_permissions_store(
                format!("{} on {:?}", operation as u32, resource),
                "none",
            ))
        }
    }

    /// Check if a resource scope matches the requested resource
    fn resource_matches(granted: &ResourceScope, requested: &ResourceScope) -> bool {
        use crate::manifest::ResourceScope as RS;

        match (granted, requested) {
            // Exact match
            (RS::StorageObject { account_id: a1 }, RS::StorageObject { account_id: a2 }) => {
                a1 == a2
            }
            (RS::AccountStorage { account_id: a1 }, RS::AccountStorage { account_id: a2 }) => {
                a1 == a2
            }
            (RS::DeviceStorage { device_id: d1 }, RS::DeviceStorage { device_id: d2 }) => d1 == d2,
            (RS::Public, RS::Public) => true,

            // Account scope grants access to its objects
            (RS::AccountStorage { .. }, RS::StorageObject { .. }) => true,

            // Everything else: no match
            _ => false,
        }
    }

    /// Verify an operation can be performed
    pub fn can_perform_operation(
        &self,
        device_id: &DeviceId,
        operation: StorageOperation,
        resource: &ResourceScope,
        current_time: u64,
    ) -> bool {
        self.verify_access(device_id, operation, resource, current_time)
            .is_ok()
    }

    /// Get all accessible resources for a device
    pub fn get_accessible_resources(
        &self,
        device_id: &DeviceId,
        operation: StorageOperation,
        current_time: u64,
    ) -> Vec<ResourceScope> {
        let tokens = match self.manager.get_tokens(device_id) {
            Some(tokens) => tokens,
            None => return Vec::new(),
        };

        let mut resources = Vec::new();

        for token in tokens {
            // Skip expired tokens
            if token.is_expired(current_time) {
                continue;
            }

            // Skip revoked tokens
            let is_revoked = token
                .delegation_chain
                .iter()
                .any(|cap_id| self.manager.is_revoked(cap_id));
            if is_revoked {
                continue;
            }

            // Collect resources accessible by this token
            for scope in token.scopes_for_operation(operation) {
                if !resources.contains(&scope) {
                    resources.push(scope);
                }
            }
        }

        resources
    }

    /// Validate token signature (would check against account threshold)
    pub fn validate_signature(
        &self,
        token: &CapabilityToken,
        _expected_threshold: u32,
    ) -> Result<()> {
        // In a full implementation, would verify the threshold signature
        // against the account's signing key with the expected threshold

        if token.signature.signature_shares.is_empty() {
            return Err(StoreErrorBuilder::access_denied("Token signature is empty"));
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::{Permission, SignatureShare, ThresholdSignature};
    use aura_types::{AccountIdExt, DeviceIdExt};

    fn create_checker_with_token(
        device_id: DeviceId,
        operation: StorageOperation,
        resource: ResourceScope,
        expires_at: Option<u64>,
    ) -> CapabilityChecker {
        use crate::access_control::capability::CapabilityManager;

        // Create a token directly with expiration
        let permission = Permission {
            operation,
            resource,
            grant_time: 1000,
            expiry: expires_at,
        };

        let mut token = CapabilityToken::new(
            device_id.clone(),
            vec![permission],
            ThresholdSignature {
                threshold: 2,
                signature_shares: vec![SignatureShare {
                    device_id: device_id.clone(),
                    share: vec![1, 2, 3],
                }],
            },
            1000,
        );

        // Set token expiration if provided
        if let Some(exp) = expires_at {
            token = token.with_expiration(exp);
        }

        // Create a custom CapabilityManager for testing
        let mut manager = CapabilityManager::new();

        // Add the token directly for testing
        manager.add_token_for_testing(device_id, token);

        CapabilityChecker::new(manager)
    }

    #[test]
    fn test_verify_access_granted() {
        let device_id = DeviceId::new_with_effects(&aura_crypto::Effects::test());
        let resource = ResourceScope::Public;
        let checker = create_checker_with_token(
            device_id.clone(),
            StorageOperation::Read,
            resource.clone(),
            Some(2000),
        );

        let result = checker.verify_access(&device_id, StorageOperation::Read, &resource, 1500);
        assert!(result.is_ok());
    }

    #[test]
    fn test_verify_access_expired() {
        let device_id = DeviceId::new_with_effects(&aura_crypto::Effects::test());
        let resource = ResourceScope::Public;
        let checker = create_checker_with_token(
            device_id.clone(),
            StorageOperation::Read,
            resource.clone(),
            Some(1000),
        );

        let result = checker.verify_access(&device_id, StorageOperation::Read, &resource, 1500);
        assert!(result.is_err());
    }

    #[test]
    fn test_verify_access_wrong_operation() {
        let device_id = DeviceId::new_with_effects(&aura_crypto::Effects::test());
        let resource = ResourceScope::Public;
        let checker = create_checker_with_token(
            device_id.clone(),
            StorageOperation::Read,
            resource.clone(),
            Some(2000),
        );

        let result = checker.verify_access(&device_id, StorageOperation::Write, &resource, 1500);
        assert!(result.is_err());
    }

    #[test]
    fn test_can_perform_operation() {
        let device_id = DeviceId::new_with_effects(&aura_crypto::Effects::test());
        let resource = ResourceScope::Public;
        let checker = create_checker_with_token(
            device_id.clone(),
            StorageOperation::Read,
            resource.clone(),
            Some(2000),
        );

        assert!(checker.can_perform_operation(&device_id, StorageOperation::Read, &resource, 1500,));

        assert!(!checker.can_perform_operation(
            &device_id,
            StorageOperation::Write,
            &resource,
            1500,
        ));
    }

    #[test]
    fn test_resource_matches_exact() {
        let resource1 = ResourceScope::Public;
        let resource2 = ResourceScope::Public;
        assert!(CapabilityChecker::resource_matches(&resource1, &resource2));
    }

    #[test]
    fn test_resource_matches_account_to_object() {
        let account_id = aura_types::AccountId::new_with_effects(&aura_crypto::Effects::test());
        let granted = ResourceScope::AccountStorage {
            account_id: account_id.clone(),
        };
        let requested = ResourceScope::StorageObject {
            account_id: account_id.clone(),
        };
        assert!(CapabilityChecker::resource_matches(&granted, &requested));
    }

    #[test]
    fn test_get_accessible_resources() {
        let device_id = DeviceId::new_with_effects(&aura_crypto::Effects::test());
        let resource = ResourceScope::Public;
        let checker = create_checker_with_token(
            device_id.clone(),
            StorageOperation::Read,
            resource.clone(),
            Some(2000),
        );

        let resources = checker.get_accessible_resources(&device_id, StorageOperation::Read, 1500);

        assert_eq!(resources.len(), 1);
        assert_eq!(resources[0], resource);
    }

    #[test]
    fn test_validate_signature() {
        let device_id = DeviceId::new_with_effects(&aura_crypto::Effects::test());
        let token = CapabilityToken::new(
            device_id.clone(),
            vec![],
            ThresholdSignature {
                threshold: 2,
                signature_shares: vec![SignatureShare {
                    device_id: device_id.clone(),
                    share: vec![1, 2, 3],
                }],
            },
            1000,
        );

        let checker = CapabilityChecker::new(CapabilityManager::new());
        assert!(checker.validate_signature(&token, 2).is_ok());
    }

    #[test]
    fn test_validate_signature_empty() {
        let device_id = DeviceId::new_with_effects(&aura_crypto::Effects::test());
        let token = CapabilityToken::new(
            device_id,
            vec![],
            ThresholdSignature {
                threshold: 2,
                signature_shares: vec![],
            },
            1000,
        );

        let checker = CapabilityChecker::new(CapabilityManager::new());
        assert!(checker.validate_signature(&token, 2).is_err());
    }
}
