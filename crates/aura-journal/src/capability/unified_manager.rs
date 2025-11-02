//! Unified capability manager
//!
//! This module provides a clean, unified interface that combines threshold-signed
//! capabilities with convergent capabilities, offering the best of both approaches
//! with clean, modern interfaces.

use super::{
    threshold_capabilities::{ThresholdCapability, ThresholdCapabilityManager},
    CapabilityError, CapabilityId, Permission, Result, StorageOperation,
};
use aura_crypto::{Ed25519VerifyingKey, Effects};
use aura_types::DeviceId;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// Simplified capability token for the unified manager (consolidated from old CapabilityManager)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnifiedCapabilityToken {
    pub id: CapabilityId,
    pub device_id: DeviceId,
    pub permissions: Vec<Permission>,
    pub issued_at: u64,
    pub expires_at: Option<u64>,
    pub delegation_chain: Vec<CapabilityId>,
}

impl UnifiedCapabilityToken {
    /// Get the capability ID (for backward compatibility with tests)
    pub fn capability_id(&self) -> CapabilityId {
        self.id
    }
}

/// Unified capability system that combines threshold and convergent capabilities
///
/// This manager provides a clean interface for modern capability management,
/// supporting both threshold-signed capabilities for high-security operations
/// and convergent capabilities for CRDT-native authorization.
#[derive(Debug, Clone)]
pub struct UnifiedCapabilityManager {
    /// Threshold capability manager for high-security operations
    threshold_manager: ThresholdCapabilityManager,

    /// Configuration for capability management
    config: UnifiedConfig,

    /// Active capability tokens indexed by device (from CapabilityManager)
    tokens: BTreeMap<DeviceId, Vec<UnifiedCapabilityToken>>,
    /// Revoked capability IDs (from CapabilityManager)
    revoked: BTreeSet<CapabilityId>,
    /// Delegation graph for cascading revocation (from CapabilityManager)
    delegation_graph: BTreeMap<CapabilityId, Vec<CapabilityId>>,
    /// Authority keys for verification (from CapabilityManager)
    authority_keys: BTreeMap<DeviceId, Ed25519VerifyingKey>,
}

/// Configuration for unified capability management
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnifiedConfig {
    /// Require threshold signatures for high-security operations
    pub require_threshold_for_admin: bool,

    /// Minimum threshold level for administrative operations
    pub admin_threshold: u16,

    /// Enable automatic cleanup of expired capabilities
    pub auto_cleanup: bool,

    /// Default capability expiration time (seconds)
    pub default_expiration: Option<u64>,
}

impl Default for UnifiedConfig {
    fn default() -> Self {
        Self {
            require_threshold_for_admin: true,
            admin_threshold: 2,
            auto_cleanup: true,
            default_expiration: Some(86400), // 24 hours
        }
    }
}

/// Result of capability verification with context
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerificationContext {
    /// Type of capability that granted access
    pub capability_type: CapabilityType,

    /// Authority level (number of signers for threshold, trust level for others)
    pub authority_level: u32,

    /// Whether the capability is near expiration
    pub near_expiration: bool,
}

/// Type of capability that granted access
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CapabilityType {
    /// Threshold-signed capability
    Threshold,

    /// Convergent capability (for future integration)
    Convergent,

    /// Group membership capability
    Group,
}

impl UnifiedCapabilityManager {
    /// Create new unified capability manager
    pub fn new(config: UnifiedConfig) -> Self {
        Self {
            threshold_manager: ThresholdCapabilityManager::new(),
            config,
            tokens: BTreeMap::new(),
            revoked: BTreeSet::new(),
            delegation_graph: BTreeMap::new(),
            authority_keys: BTreeMap::new(),
        }
    }

    /// Grant threshold capability
    pub fn grant_threshold_capability(&mut self, capability: ThresholdCapability) -> Result<()> {
        self.threshold_manager.grant_capability(capability)
    }

    /// Verify permission with full context
    pub fn verify_permission(
        &self,
        device_id: &DeviceId,
        permission: &Permission,
        effects: &Effects,
    ) -> Result<VerificationContext> {
        let current_time = effects
            .now()
            .map_err(|e| CapabilityError::CryptoError(format!("Failed to get time: {:?}", e)))?;

        // Try threshold capabilities first
        if let Ok(threshold_cap) =
            self.threshold_manager
                .verify_permission(device_id, permission, current_time)
        {
            let near_expiration = if let Some(expires_at) = threshold_cap.expires_at {
                (expires_at - current_time) < 3600 // Within 1 hour
            } else {
                false
            };

            return Ok(VerificationContext {
                capability_type: CapabilityType::Threshold,
                authority_level: threshold_cap.authority_level() as u32,
                near_expiration,
            });
        }

        // TODO: Add convergent capability verification here
        // TODO: Add group membership verification here

        Err(CapabilityError::AuthorizationError(
            "No valid capabilities found".to_string(),
        ))
    }

    /// Check if permission requires administrative privileges
    pub fn requires_admin_privileges(&self, permission: &Permission) -> bool {
        match permission {
            Permission::Storage { operation, .. } => {
                matches!(operation, super::StorageOperation::Delete)
            }
            Permission::Communication { .. } => {
                // Administrative communication operations (future)
                false
            }
            Permission::Relay { trust_level, .. } => trust_level == "admin",
            Permission::DeviceAuth(_) => false, // Device auth doesn't require admin
        }
    }

    /// Verify permission with administrative privilege checking
    pub fn verify_admin_permission(
        &self,
        device_id: &DeviceId,
        permission: &Permission,
        effects: &Effects,
    ) -> Result<VerificationContext> {
        let context = self.verify_permission(device_id, permission, effects)?;

        // Check if admin privileges are required
        if self.requires_admin_privileges(permission)
            && self.config.require_threshold_for_admin {
                match context.capability_type {
                    CapabilityType::Threshold => {
                        if context.authority_level < self.config.admin_threshold as u32 {
                            return Err(CapabilityError::AuthorizationError(
                                "Insufficient threshold signatures for admin operation".to_string(),
                            ));
                        }
                    }
                    _ => {
                        return Err(CapabilityError::AuthorizationError(
                            "Admin operations require threshold signatures".to_string(),
                        ));
                    }
                }
            }

        Ok(context)
    }

    /// Get capability statistics across all managers
    pub fn stats(&self) -> UnifiedStats {
        let threshold_stats = self.threshold_manager.stats();
        let total_tokens: usize = self.tokens.values().map(|tokens| tokens.len()).sum();

        UnifiedStats {
            threshold_capabilities: threshold_stats.total_capabilities,
            convergent_capabilities: total_tokens,
            group_capabilities: 0, // TODO: Add when group manager is integrated
            revoked_count: self.revoked.len() + threshold_stats.revoked_count,
            device_count: self.tokens.len().max(threshold_stats.device_count),
        }
    }

    /// Clean up expired capabilities
    pub fn cleanup(&mut self, effects: &Effects) -> Result<u32> {
        if !self.config.auto_cleanup {
            return Ok(0);
        }

        let current_time = effects
            .now()
            .map_err(|e| CapabilityError::CryptoError(format!("Failed to get time: {:?}", e)))?;

        let before_stats = self.threshold_manager.stats();
        self.threshold_manager.cleanup(current_time);
        let after_stats = self.threshold_manager.stats();

        let threshold_cleaned = before_stats.total_capabilities - after_stats.total_capabilities;

        // Clean up expired token capabilities
        let before_tokens: usize = self.tokens.values().map(|tokens| tokens.len()).sum();
        self.cleanup_expired_tokens(current_time);
        let after_tokens: usize = self.tokens.values().map(|tokens| tokens.len()).sum();

        let token_cleaned = before_tokens - after_tokens;

        Ok((threshold_cleaned + token_cleaned) as u32)
    }

    /// Register trusted key package for threshold verification
    pub fn register_key_package(
        &mut self,
        name: String,
        key_package: super::threshold_capabilities::PublicKeyPackage,
    ) {
        self.threshold_manager
            .register_key_package(name, key_package);
    }

    /// Get configuration
    pub fn config(&self) -> &UnifiedConfig {
        &self.config
    }

    /// Update configuration
    pub fn update_config(&mut self, config: UnifiedConfig) {
        self.config = config;
    }
}

/// Unified capability statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnifiedStats {
    pub threshold_capabilities: usize,
    pub convergent_capabilities: usize,
    pub group_capabilities: usize,
    pub revoked_count: usize,
    pub device_count: usize,
}

/// Backward compatibility methods for old CapabilityManager API
impl UnifiedCapabilityManager {
    /// Verify storage access using the old CapabilityManager API (for backward compatibility)
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

        self.verify_permission_simple(device_id, &required, current_time)
    }

    /// Simple permission verification for backward compatibility
    fn verify_permission_simple(
        &self,
        device_id: &DeviceId,
        permission: &Permission,
        current_time: u64,
    ) -> Result<()> {
        // Check if device has any capabilities
        if let Some(tokens) = self.tokens.get(device_id) {
            for token in tokens {
                // Check if not revoked
                if self.revoked.contains(&token.id) {
                    continue;
                }

                // Check expiration
                if let Some(expires_at) = token.expires_at {
                    if current_time >= expires_at {
                        continue;
                    }
                }

                // Check if permission matches
                if token
                    .permissions
                    .iter()
                    .any(|p| permissions_match(p, permission))
                {
                    return Ok(());
                }
            }
        }

        Err(CapabilityError::AuthorizationError(
            "No valid capabilities found".to_string(),
        ))
    }
}

/// Check if two permissions match
fn permissions_match(granted: &Permission, required: &Permission) -> bool {
    match (granted, required) {
        (
            Permission::Storage {
                operation: g_op,
                resource: g_res,
            },
            Permission::Storage {
                operation: r_op,
                resource: r_res,
            },
        ) => g_op == r_op && (g_res == "*" || g_res == r_res),
        (
            Permission::Communication {
                operation: g_op,
                relationship: g_rel,
            },
            Permission::Communication {
                operation: r_op,
                relationship: r_rel,
            },
        ) => g_op == r_op && (g_rel == "*" || g_rel == r_rel),
        (
            Permission::Relay {
                operation: g_op,
                trust_level: g_level,
            },
            Permission::Relay {
                operation: r_op,
                trust_level: r_level,
            },
        ) => g_op == r_op && g_level == r_level,
        _ => false,
    }
}

/// Convenience methods for common operations
impl UnifiedCapabilityManager {
    /// Verify storage permission
    pub fn verify_storage_access(
        &self,
        device_id: &DeviceId,
        operation: super::StorageOperation,
        resource: &str,
        effects: &Effects,
    ) -> Result<VerificationContext> {
        let permission = Permission::Storage {
            operation: operation.clone(),
            resource: resource.to_string(),
        };

        match operation {
            super::StorageOperation::Delete => {
                // Delete operations require admin verification
                self.verify_admin_permission(device_id, &permission, effects)
            }
            _ => self.verify_permission(device_id, &permission, effects),
        }
    }

    /// Verify communication permission
    pub fn verify_communication_access(
        &self,
        device_id: &DeviceId,
        operation: super::CommunicationOperation,
        relationship: &str,
        effects: &Effects,
    ) -> Result<VerificationContext> {
        let permission = Permission::Communication {
            operation,
            relationship: relationship.to_string(),
        };

        self.verify_permission(device_id, &permission, effects)
    }

    /// Verify relay permission with trust level checking
    pub fn verify_relay_access(
        &self,
        device_id: &DeviceId,
        operation: super::RelayOperation,
        trust_level: &str,
        effects: &Effects,
    ) -> Result<VerificationContext> {
        let permission = Permission::Relay {
            operation,
            trust_level: trust_level.to_string(),
        };

        if trust_level == "admin" {
            self.verify_admin_permission(device_id, &permission, effects)
        } else {
            self.verify_permission(device_id, &permission, effects)
        }
    }

    /// Register authority key for verification (from CapabilityManager)
    pub fn register_authority(&mut self, device_id: DeviceId, key: Ed25519VerifyingKey) {
        self.authority_keys.insert(device_id, key);
    }

    /// Grant capability using the CapabilityManager API signature
    pub fn grant_capability(
        &mut self,
        grant: super::manager::CapabilityGrant,
        signing_key: &aura_crypto::Ed25519SigningKey,
        effects: &Effects,
    ) -> Result<super::unified::CapabilityToken> {
        // Create a simple CapabilityToken that mimics the old behavior
        let token = super::unified::CapabilityToken::new(
            grant.device_id,
            grant.permissions.clone(),
            grant.delegation_chain.clone(),
            signing_key,
            effects,
        )
        .map_err(CapabilityError::CryptoError)?;

        // Store in our internal format
        let unified_token = UnifiedCapabilityToken {
            id: CapabilityId::random(),
            device_id: grant.device_id,
            permissions: grant.permissions,
            issued_at: grant.issued_at,
            expires_at: grant.expires_at,
            delegation_chain: grant.delegation_chain,
        };

        self.tokens
            .entry(grant.device_id)
            .or_default()
            .push(unified_token);
        Ok(token)
    }

    /// Grant capability token with simple parameters (new API)
    pub fn grant_capability_simple(
        &mut self,
        device_id: DeviceId,
        permissions: Vec<Permission>,
        expires_at: Option<u64>,
        effects: &Effects,
    ) -> Result<UnifiedCapabilityToken> {
        let current_time = effects
            .now()
            .map_err(|e| CapabilityError::CryptoError(format!("Failed to get time: {:?}", e)))?;

        let token = UnifiedCapabilityToken {
            id: CapabilityId::random(),
            device_id,
            permissions,
            issued_at: current_time,
            expires_at,
            delegation_chain: vec![],
        };

        self.tokens
            .entry(device_id)
            .or_default()
            .push(token.clone());
        Ok(token)
    }

    /// Delegate capability (from CapabilityManager)
    pub fn delegate_capability(
        &mut self,
        parent_capability_id: CapabilityId,
        target_device_id: DeviceId,
        permissions: Vec<Permission>,
        expires_at: Option<u64>,
        effects: &Effects,
    ) -> Result<UnifiedCapabilityToken> {
        let current_time = effects
            .now()
            .map_err(|e| CapabilityError::CryptoError(format!("Failed to get time: {:?}", e)))?;

        let new_capability_id = CapabilityId::random();
        let mut delegation_chain = vec![parent_capability_id];

        // Find parent's delegation chain if it exists
        for tokens in self.tokens.values() {
            if let Some(parent_token) = tokens.iter().find(|t| t.id == parent_capability_id) {
                delegation_chain = parent_token.delegation_chain.clone();
                delegation_chain.push(parent_capability_id);
                break;
            }
        }

        let token = UnifiedCapabilityToken {
            id: new_capability_id,
            device_id: target_device_id,
            permissions,
            issued_at: current_time,
            expires_at,
            delegation_chain,
        };

        // Update delegation graph
        self.delegation_graph
            .entry(parent_capability_id)
            .or_default()
            .push(new_capability_id);

        self.tokens
            .entry(target_device_id)
            .or_default()
            .push(token.clone());
        Ok(token)
    }

    /// Revoke capability with cascading delegation revocation (from CapabilityManager)
    pub fn revoke_capability(&mut self, capability_id: CapabilityId) -> Result<()> {
        self.revoked.insert(capability_id);

        // Recursively revoke all delegated capabilities
        if let Some(delegated_caps) = self.delegation_graph.remove(&capability_id) {
            for delegated_cap in delegated_caps {
                self.revoke_capability(delegated_cap)?;
            }
        }

        // Remove from active tokens
        for tokens in self.tokens.values_mut() {
            tokens.retain(|token| token.id != capability_id);
        }

        Ok(())
    }

    /// Get capabilities for device (from CapabilityManager)
    pub fn get_capabilities(&self, device_id: &DeviceId) -> Vec<UnifiedCapabilityToken> {
        self.tokens.get(device_id).cloned().unwrap_or_default()
    }

    /// Cleanup expired capabilities (from CapabilityManager)
    pub fn cleanup_expired_tokens(&mut self, current_time: u64) {
        for tokens in self.tokens.values_mut() {
            tokens.retain(|token| {
                if let Some(expires_at) = token.expires_at {
                    expires_at > current_time
                } else {
                    true // No expiration
                }
            });
        }
    }
}

impl Default for UnifiedCapabilityManager {
    fn default() -> Self {
        Self::new(UnifiedConfig::default())
    }
}

#[cfg(test)]
mod tests {
    use super::super::threshold_capabilities::{
        ParticipantId, PublicKeyPackage, ThresholdCapability, ThresholdSignature,
    };
    use super::*;
    use aura_crypto::{Ed25519Signature, Ed25519SigningKey};
    use std::num::NonZeroU16;
    use uuid::Uuid;

    fn test_effects() -> Effects {
        Effects::for_test("unified_manager_test")
    }

    fn test_device_id() -> DeviceId {
        DeviceId(Uuid::new_v4())
    }

    fn mock_threshold_capability(device_id: DeviceId, admin: bool) -> ThresholdCapability {
        let effects = test_effects();
        let signing_key = aura_crypto::Ed25519SigningKey::from_bytes(&[1u8; 32]);

        let permissions = if admin {
            vec![Permission::Storage {
                operation: super::super::StorageOperation::Delete,
                resource: "*".to_string(),
            }]
        } else {
            vec![Permission::Storage {
                operation: super::super::StorageOperation::Read,
                resource: "test/*".to_string(),
            }]
        };

        let authorization = ThresholdSignature {
            signature: aura_crypto::Ed25519Signature::from_bytes(&[0u8; 64]),
            signers: vec![
                ParticipantId::new(NonZeroU16::new(1).unwrap()),
                ParticipantId::new(NonZeroU16::new(2).unwrap()),
            ],
        };

        let public_key_package = PublicKeyPackage {
            group_public: signing_key.verifying_key(),
            threshold: 2,
            total_participants: 3,
        };

        ThresholdCapability::new(
            device_id,
            permissions,
            authorization,
            public_key_package,
            &effects,
        )
        .unwrap()
    }

    #[test]
    fn test_unified_manager_creation() {
        let manager = UnifiedCapabilityManager::default();
        assert!(manager.config().require_threshold_for_admin);
        assert_eq!(manager.config().admin_threshold, 2);
    }

    #[test]
    fn test_admin_privilege_detection() {
        let manager = UnifiedCapabilityManager::default();

        let delete_permission = Permission::Storage {
            operation: super::super::StorageOperation::Delete,
            resource: "test.txt".to_string(),
        };

        let read_permission = Permission::Storage {
            operation: super::super::StorageOperation::Read,
            resource: "test.txt".to_string(),
        };

        let admin_relay = Permission::Relay {
            operation: super::super::RelayOperation::Forward,
            trust_level: "admin".to_string(),
        };

        assert!(manager.requires_admin_privileges(&delete_permission));
        assert!(!manager.requires_admin_privileges(&read_permission));
        assert!(manager.requires_admin_privileges(&admin_relay));
    }

    #[test]
    fn test_verification_context() {
        let device_id = test_device_id();
        let capability = mock_threshold_capability(device_id, false);

        assert_eq!(capability.authority_level(), 2);
        assert!(!capability.is_expired(test_effects().now().unwrap()));
    }

    #[test]
    fn test_stats() {
        let manager = UnifiedCapabilityManager::default();
        let stats = manager.stats();

        assert_eq!(stats.threshold_capabilities, 0);
        assert_eq!(stats.device_count, 0);
    }

    #[test]
    fn test_config_update() {
        let mut manager = UnifiedCapabilityManager::default();

        let mut new_config = UnifiedConfig::default();
        new_config.admin_threshold = 3;
        new_config.require_threshold_for_admin = false;

        manager.update_config(new_config);

        assert_eq!(manager.config().admin_threshold, 3);
        assert!(!manager.config().require_threshold_for_admin);
    }
}
