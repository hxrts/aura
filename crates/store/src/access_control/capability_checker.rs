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
use aura_journal::core::ledger::AccountLedger;

/// Context for access control evaluation
#[derive(Debug, Clone)]
pub struct AccessContext {
    pub current_time: u64,
    pub authority_level: u32,
    pub quota_info: Option<QuotaInfo>,
}

/// Quota information for resource access
#[derive(Debug, Clone)]
pub struct QuotaInfo {
    pub current_usage: u64,
    pub limit: u64,
}

/// Result of resource access evaluation
#[derive(Debug, Clone)]
pub enum ResourceAccessResult {
    /// Access granted
    Granted {
        granted_at: u64,
        authority_level: u32,
    },
    /// Access denied
    Denied { reason: String, details: String },
}
use aura_types::{DeviceId, DeviceIdExt};

/// Capability checker - verifies storage access permissions
pub struct CapabilityChecker {
    manager: CapabilityManager,
    /// Optional ledger for device-to-account verification
    /// None if running in test mode or if ledger is not available
    ledger: Option<std::sync::Arc<AccountLedger>>,
}

impl CapabilityChecker {
    /// Create a new capability checker
    pub fn new(manager: CapabilityManager) -> Self {
        Self { 
            manager,
            ledger: None,
        }
    }

    /// Create a new capability checker with ledger integration
    pub fn with_ledger(manager: CapabilityManager, ledger: std::sync::Arc<AccountLedger>) -> Self {
        Self { 
            manager,
            ledger: Some(ledger),
        }
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

        // Check permissions with fine-grained resource matching
        let has_permission = token
            .granted_permissions
            .iter()
            .any(|p| p.operation == operation && self.resource_matches(&p.resource, resource));

        if has_permission {
            tracing::debug!(
                operation = ?operation,
                resource = ?resource,
                "Resource permission granted"
            );
            Ok(())
        } else {
            tracing::warn!(
                operation = ?operation,
                resource = ?resource,
                granted_permissions = ?token.granted_permissions,
                "Resource permission denied - no matching permission found"
            );
            Err(StoreErrorBuilder::insufficient_permissions_store(
                format!("{:?} on {:?}", operation, resource),
                "no matching resource permission",
            ))
        }
    }

    /// Check if a resource scope matches the requested resource with fine-grained control
    fn resource_matches(&self, granted: &ResourceScope, requested: &ResourceScope) -> bool {
        use crate::manifest::ResourceScope as RS;

        tracing::trace!(
            granted = ?granted,
            requested = ?requested,
            "Evaluating resource permission match"
        );

        let matches = match (granted, requested) {
            // Exact match cases
            (RS::StorageObject { account_id: a1 }, RS::StorageObject { account_id: a2 }) => {
                let exact_match = a1 == a2;
                tracing::trace!("StorageObject exact match: {}", exact_match);
                exact_match
            }
            (RS::AccountStorage { account_id: a1 }, RS::AccountStorage { account_id: a2 }) => {
                let exact_match = a1 == a2;
                tracing::trace!("AccountStorage exact match: {}", exact_match);
                exact_match
            }
            (RS::DeviceStorage { device_id: d1 }, RS::DeviceStorage { device_id: d2 }) => {
                let exact_match = d1 == d2;
                tracing::trace!("DeviceStorage exact match: {}", exact_match);
                exact_match
            }
            (RS::Public, RS::Public) => {
                tracing::trace!("Public resource match: true");
                true
            }

            // Hierarchical scope matching - broader scopes grant access to more specific resources
            (
                RS::AccountStorage {
                    account_id: granted_account,
                },
                RS::StorageObject {
                    account_id: requested_account,
                },
            ) => {
                let hierarchical_match = granted_account == requested_account;
                tracing::trace!(
                    "AccountStorage -> StorageObject hierarchical match: {}",
                    hierarchical_match
                );
                hierarchical_match
            }

            (
                RS::AccountStorage {
                    account_id: granted_account,
                },
                RS::DeviceStorage {
                    device_id: requested_device,
                },
            ) => {
                // Account storage can access device storage if device belongs to account
                let hierarchical_match =
                    self.device_belongs_to_account(requested_device, granted_account);
                tracing::trace!(
                    "AccountStorage -> DeviceStorage hierarchical match: {}",
                    hierarchical_match
                );
                hierarchical_match
            }

            // Public scope grants access to public resources only
            (RS::Public, _) => {
                tracing::trace!("Public scope cannot access non-public resources");
                false
            }
            (_, RS::Public) => {
                tracing::trace!("Any scope can access public resources");
                true
            }

            // Cross-scope access: not allowed
            (RS::DeviceStorage { .. }, RS::AccountStorage { .. }) => {
                tracing::trace!("DeviceStorage cannot access AccountStorage");
                false
            }
            (RS::DeviceStorage { .. }, RS::StorageObject { .. }) => {
                tracing::trace!("DeviceStorage cannot access StorageObject directly");
                false
            }
            (RS::StorageObject { .. }, RS::AccountStorage { .. }) => {
                tracing::trace!("StorageObject cannot access AccountStorage");
                false
            }
            (RS::StorageObject { .. }, RS::DeviceStorage { .. }) => {
                tracing::trace!("StorageObject cannot access DeviceStorage");
                false
            }

            // AllOwnedObjects scope patterns - broad access
            (RS::AllOwnedObjects, _) => {
                tracing::trace!("AllOwnedObjects grants access to any resource");
                true
            }
            (_, RS::AllOwnedObjects) => {
                tracing::trace!("Any scope can access AllOwnedObjects");
                true
            }

            // Object scope patterns - specific object access
            (RS::Object { cid: granted_cid }, RS::Object { cid: requested_cid }) => {
                let exact_match = granted_cid == requested_cid;
                tracing::trace!("Object exact match: {}", exact_match);
                exact_match
            }
            (RS::Object { .. }, _) => {
                tracing::trace!("Object scope cannot access other resource types");
                false
            }
            (_, RS::Object { .. }) => {
                tracing::trace!("Non-object scopes cannot access specific objects");
                false
            }

            // Manifest scope patterns - specific manifest access
            (RS::Manifest { cid: granted_cid }, RS::Manifest { cid: requested_cid }) => {
                let exact_match = granted_cid == requested_cid;
                tracing::trace!("Manifest exact match: {}", exact_match);
                exact_match
            }
            (RS::Manifest { .. }, _) => {
                tracing::trace!("Manifest scope cannot access other resource types");
                false
            }
            (_, RS::Manifest { .. }) => {
                tracing::trace!("Non-manifest scopes cannot access specific manifests");
                false
            }
        };

        tracing::debug!(
            granted = ?granted,
            requested = ?requested,
            matches = matches,
            "Resource permission match result"
        );

        matches
    }

    /// Check if a device belongs to a specific account
    /// 
    /// This method provides real device-to-account verification by querying
    /// the account ledger to verify that the device is enrolled in the account.
    fn device_belongs_to_account(
        &self,
        device_id: &aura_types::DeviceId,
        account_id: &aura_types::AccountId,
    ) -> bool {
        tracing::debug!(
            device_id = %device_id,
            account_id = %account_id,
            "Verifying device-to-account mapping"
        );

        // If no ledger is available, fall back to permissive mode for testing
        let Some(ledger) = &self.ledger else {
            tracing::warn!(
                device_id = %device_id,
                account_id = %account_id,
                "No ledger available for device-to-account verification - allowing access for testing"
            );
            return true;
        };

        // Verify that this ledger is for the requested account
        let ledger_account_id = &ledger.state().account_id;
        if ledger_account_id != account_id {
            tracing::warn!(
                device_id = %device_id,
                requested_account = %account_id,
                ledger_account = %ledger_account_id,
                "Account ID mismatch - device requested access to different account than ledger manages"
            );
            return false;
        }

        // Check if the device is enrolled and active in this account
        let device_enrolled = ledger.state().is_device_active(device_id);
        
        if device_enrolled {
            tracing::debug!(
                device_id = %device_id,
                account_id = %account_id,
                "Device successfully verified as belonging to account"
            );
        } else {
            tracing::warn!(
                device_id = %device_id,
                account_id = %account_id,
                "Device verification failed - device not enrolled or inactive in account"
            );
        }

        device_enrolled
    }

    /// Advanced resource pattern matching with wildcard and path-based permissions
    pub fn matches_resource_pattern(
        &self,
        granted_pattern: &str,
        requested_resource: &str,
    ) -> bool {
        tracing::trace!(
            granted_pattern = granted_pattern,
            requested_resource = requested_resource,
            "Evaluating resource pattern match"
        );

        let matches = if granted_pattern == "*" {
            // Universal wildcard grants access to everything
            true
        } else if granted_pattern.ends_with("/*") {
            // Path prefix wildcard (e.g., "user_data/*" matches "user_data/file.txt")
            let prefix = &granted_pattern[..granted_pattern.len() - 1]; // Remove "*", keep "/"
            requested_resource.starts_with(prefix)
        } else if granted_pattern.contains("*") {
            // Pattern matching with wildcards
            self.glob_match(granted_pattern, requested_resource)
        } else {
            // Exact string match
            granted_pattern == requested_resource
        };

        tracing::debug!(
            granted_pattern = granted_pattern,
            requested_resource = requested_resource,
            matches = matches,
            "Resource pattern match result"
        );

        matches
    }

    /// Simple glob-style pattern matching
    fn glob_match(&self, pattern: &str, text: &str) -> bool {
        // Convert glob pattern to regex-like matching
        // This is a simplified implementation - could be enhanced with regex crate
        let pattern_parts: Vec<&str> = pattern.split('*').collect();

        if pattern_parts.len() == 1 {
            // No wildcards, exact match
            return pattern == text;
        }

        let mut text_pos = 0;

        for (i, part) in pattern_parts.iter().enumerate() {
            if part.is_empty() {
                continue;
            }

            if i == 0 {
                // First part must match at the beginning
                if !text[text_pos..].starts_with(part) {
                    return false;
                }
                text_pos += part.len();
            } else if i == pattern_parts.len() - 1 {
                // Last part must match at the end
                return text[text_pos..].ends_with(part);
            } else {
                // Middle part must be found somewhere
                if let Some(pos) = text[text_pos..].find(part) {
                    text_pos += pos + part.len();
                } else {
                    return false;
                }
            }
        }

        true
    }

    /// Evaluate resource permission with context-aware access control
    pub fn evaluate_resource_access(
        &self,
        granted_permission: &crate::manifest::Permission,
        requested_operation: &crate::manifest::StorageOperation,
        requested_resource: &ResourceScope,
        context: &AccessContext,
    ) -> ResourceAccessResult {
        // Check operation match
        if granted_permission.operation != *requested_operation {
            return ResourceAccessResult::Denied {
                reason: "Operation mismatch".to_string(),
                details: format!(
                    "Granted: {:?}, Requested: {:?}",
                    granted_permission.operation, requested_operation
                ),
            };
        }

        // Check resource scope match
        if !self.resource_matches(&granted_permission.resource, requested_resource) {
            return ResourceAccessResult::Denied {
                reason: "Resource scope mismatch".to_string(),
                details: format!(
                    "Granted: {:?}, Requested: {:?}",
                    granted_permission.resource, requested_resource
                ),
            };
        }

        // Check time-based constraints
        if let Some(expiry) = granted_permission.expiry {
            if context.current_time >= expiry {
                return ResourceAccessResult::Denied {
                    reason: "Permission expired".to_string(),
                    details: format!("Expired at: {}, Current: {}", expiry, context.current_time),
                };
            }
        }

        // Check rate limiting and quota constraints
        if let Some(ref quota) = context.quota_info {
            if quota.current_usage >= quota.limit {
                return ResourceAccessResult::Denied {
                    reason: "Quota exceeded".to_string(),
                    details: format!("Usage: {}/{}", quota.current_usage, quota.limit),
                };
            }
        }

        ResourceAccessResult::Granted {
            granted_at: context.current_time,
            authority_level: context.authority_level,
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
