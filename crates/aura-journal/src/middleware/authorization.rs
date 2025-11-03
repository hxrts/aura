//! Authorization middleware for journal operations

use super::{JournalMiddleware, JournalHandler, JournalContext};
use crate::error::{Error, Result};
use crate::operations::JournalOperation;
use aura_types::{DeviceId, CanonicalPermission};
use std::collections::HashMap;

/// Authorization middleware that validates permissions for journal operations
pub struct AuthorizationMiddleware {
    /// Permission checker
    checker: Box<dyn PermissionChecker>,
    
    /// Configuration
    config: AuthorizationConfig,
}

impl AuthorizationMiddleware {
    /// Create new authorization middleware
    pub fn new(
        checker: Box<dyn PermissionChecker>,
        config: AuthorizationConfig,
    ) -> Self {
        Self { checker, config }
    }
    
    /// Create middleware with default in-memory permission checker
    pub fn with_default_checker(config: AuthorizationConfig) -> Self {
        Self::new(Box::new(InMemoryPermissionChecker::new()), config)
    }
}

impl JournalMiddleware for AuthorizationMiddleware {
    fn process(
        &self,
        operation: JournalOperation,
        context: &JournalContext,
        next: &dyn JournalHandler,
    ) -> Result<serde_json::Value> {
        // Skip authorization if disabled
        if !self.config.enable_authorization {
            return next.handle(operation, context);
        }
        
        // Determine required permission for the operation
        let required_permission = self.get_required_permission(&operation)?;
        
        // Check if the device has the required permission
        let has_permission = self.checker.check_permission(
            &context.device_id,
            &context.account_id,
            &required_permission,
        )?;
        
        if !has_permission {
            return Err(Error::invalid_operation(format!(
                "Device {} lacks permission {:?} for operation {:?}",
                context.device_id,
                required_permission,
                operation
            )));
        }
        
        // Permission granted, proceed with operation
        next.handle(operation, context)
    }
    
    fn name(&self) -> &str {
        "authorization"
    }
}

impl AuthorizationMiddleware {
    fn get_required_permission(&self, operation: &JournalOperation) -> Result<CanonicalPermission> {
        match operation {
            JournalOperation::AddDevice { .. } => Ok(CanonicalPermission::Admin),
            JournalOperation::RemoveDevice { .. } => Ok(CanonicalPermission::Admin),
            JournalOperation::AddGuardian { .. } => Ok(CanonicalPermission::Admin),
            JournalOperation::IncrementEpoch => Ok(CanonicalPermission::StorageWrite),
            JournalOperation::GetDevices => Ok(CanonicalPermission::StorageRead),
            JournalOperation::GetEpoch => Ok(CanonicalPermission::StorageRead),
        }
    }
}

/// Configuration for authorization middleware
#[derive(Debug, Clone)]
pub struct AuthorizationConfig {
    /// Whether authorization is enabled
    pub enable_authorization: bool,
    
    /// Whether to allow admin bypass
    pub allow_admin_bypass: bool,
    
    /// Default permissions for new devices
    pub default_device_permissions: Vec<CanonicalPermission>,
}

impl Default for AuthorizationConfig {
    fn default() -> Self {
        Self {
            enable_authorization: true,
            allow_admin_bypass: true,
            default_device_permissions: vec![
                CanonicalPermission::StorageRead,
                CanonicalPermission::StorageWrite,
            ],
        }
    }
}

/// Trait for checking permissions
pub trait PermissionChecker: Send + Sync {
    /// Check if a device has a specific permission on an account
    fn check_permission(
        &self,
        device_id: &DeviceId,
        account_id: &aura_types::AccountId,
        permission: &CanonicalPermission,
    ) -> Result<bool>;
    
    /// Grant a permission to a device
    fn grant_permission(
        &mut self,
        device_id: &DeviceId,
        account_id: &aura_types::AccountId,
        permission: CanonicalPermission,
    ) -> Result<()>;
    
    /// Revoke a permission from a device
    fn revoke_permission(
        &mut self,
        device_id: &DeviceId,
        account_id: &aura_types::AccountId,
        permission: &CanonicalPermission,
    ) -> Result<()>;
}

/// Simple in-memory permission checker for testing and development
pub struct InMemoryPermissionChecker {
    /// Permissions keyed by (account_id, device_id)
    permissions: HashMap<(String, String), Vec<CanonicalPermission>>,
}

impl InMemoryPermissionChecker {
    /// Create a new in-memory permission checker
    pub fn new() -> Self {
        Self {
            permissions: HashMap::new(),
        }
    }
    
    /// Add default permissions for a device
    pub fn add_device_with_permissions(
        &mut self,
        device_id: &DeviceId,
        account_id: &aura_types::AccountId,
        permissions: Vec<CanonicalPermission>,
    ) {
        let key = (account_id.to_string(), device_id.to_string());
        self.permissions.insert(key, permissions);
    }
}

impl Default for InMemoryPermissionChecker {
    fn default() -> Self {
        Self::new()
    }
}

impl PermissionChecker for InMemoryPermissionChecker {
    fn check_permission(
        &self,
        device_id: &DeviceId,
        account_id: &aura_types::AccountId,
        permission: &CanonicalPermission,
    ) -> Result<bool> {
        let key = (account_id.to_string(), device_id.to_string());
        
        if let Some(device_permissions) = self.permissions.get(&key) {
            // Check for exact permission or admin permission
            Ok(device_permissions.contains(permission) || 
               device_permissions.contains(&CanonicalPermission::Admin))
        } else {
            // Device not found, deny access
            Ok(false)
        }
    }
    
    fn grant_permission(
        &mut self,
        device_id: &DeviceId,
        account_id: &aura_types::AccountId,
        permission: CanonicalPermission,
    ) -> Result<()> {
        let key = (account_id.to_string(), device_id.to_string());
        
        self.permissions
            .entry(key)
            .or_insert_with(Vec::new)
            .push(permission);
        
        Ok(())
    }
    
    fn revoke_permission(
        &mut self,
        device_id: &DeviceId,
        account_id: &aura_types::AccountId,
        permission: &CanonicalPermission,
    ) -> Result<()> {
        let key = (account_id.to_string(), device_id.to_string());
        
        if let Some(device_permissions) = self.permissions.get_mut(&key) {
            device_permissions.retain(|p| p != permission);
        }
        
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::middleware::handler::NoOpHandler;
    use crate::operations::JournalOperation;
    use aura_types::{AccountIdExt, DeviceIdExt};
    use aura_crypto::Effects;
    
    #[test]
    fn test_authorization_middleware_allows_permitted_operation() {
        let effects = Effects::test(42);
        let account_id = aura_types::AccountId::new_with_effects(&effects);
        let device_id = aura_types::DeviceId::new_with_effects(&effects);
        
        let mut checker = InMemoryPermissionChecker::new();
        checker.add_device_with_permissions(
            &device_id,
            &account_id,
            vec![CanonicalPermission::StorageRead],
        );
        
        let middleware = AuthorizationMiddleware::new(
            Box::new(checker),
            AuthorizationConfig::default(),
        );
        let handler = NoOpHandler;
        let context = JournalContext::new(account_id, device_id, "test".to_string());
        let operation = JournalOperation::GetEpoch;
        
        let result = middleware.process(operation, &context, &handler);
        assert!(result.is_ok());
    }
    
    #[test]
    fn test_authorization_middleware_denies_unpermitted_operation() {
        let effects = Effects::test(42);
        let account_id = aura_types::AccountId::new_with_effects(&effects);
        let device_id = aura_types::DeviceId::new_with_effects(&effects);
        
        let checker = InMemoryPermissionChecker::new();
        // Don't add any permissions for the device
        
        let middleware = AuthorizationMiddleware::new(
            Box::new(checker),
            AuthorizationConfig::default(),
        );
        let handler = NoOpHandler;
        let context = JournalContext::new(account_id, device_id, "test".to_string());
        let operation = JournalOperation::GetEpoch;
        
        let result = middleware.process(operation, &context, &handler);
        assert!(result.is_err());
    }
    
    #[test]
    fn test_admin_permission_allows_all_operations() {
        let effects = Effects::test(42);
        let account_id = aura_types::AccountId::new_with_effects(&effects);
        let device_id = aura_types::DeviceId::new_with_effects(&effects);
        
        let mut checker = InMemoryPermissionChecker::new();
        checker.add_device_with_permissions(
            &device_id,
            &account_id,
            vec![CanonicalPermission::Admin],
        );
        
        let middleware = AuthorizationMiddleware::new(
            Box::new(checker),
            AuthorizationConfig::default(),
        );
        let handler = NoOpHandler;
        let context = JournalContext::new(account_id, device_id, "test".to_string());
        
        // Test admin operation (requires Admin permission)
        let operation = JournalOperation::AddDevice {
            device: DeviceMetadata {
                device_id: aura_types::DeviceId::new_with_effects(&effects),
                device_name: "test".to_string(),
                device_type: DeviceType::Native,
                public_key: aura_crypto::Ed25519VerifyingKey::from_bytes(&effects.random_bytes::<32>()).unwrap(),
                added_at: 1000,
                last_seen: 1000,
                dkd_commitment_proofs: std::collections::BTreeMap::new(),
                next_nonce: 0,
                used_nonces: std::collections::BTreeSet::new(),
                key_share_epoch: 0,
            },
        };
        
        let result = middleware.process(operation, &context, &handler);
        assert!(result.is_ok());
    }
}