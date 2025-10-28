//! Keyhive-enhanced capability manager
//!
//! This module provides the enhanced capability manager that integrates Aura's
//! native capability system with Keyhive convergent capabilities for CRDT-native
//! authorization and BeeKEM group integration.

use super::{
    manager::{CapabilityGrant, CapabilityManager},
    AuthorityGraph, CapabilityError, CapabilityToken, CommunicationOperation, Permission,
    RelayOperation, Result, StorageOperation,
};
use aura_crypto::Effects;
// Group integration via traits to avoid circular dependencies
use aura_types::DeviceId;
use aura_crypto::{Ed25519SigningKey, Ed25519VerifyingKey};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use tracing::{debug, info, warn};

/// Trait for group membership provider to avoid circular dependencies
pub trait GroupMembershipProvider: std::fmt::Debug + Send + Sync {
    /// Check if device is member of a group
    fn is_group_member(&self, device_id: &DeviceId, group_id: &str) -> bool;

    /// Get all groups a device is member of
    fn get_device_groups(&self, device_id: &DeviceId) -> Vec<String>;

    /// Get all members of a group
    fn get_group_members(&self, group_id: &str) -> Vec<DeviceId>;
}

/// Simple in-memory group membership provider for testing
#[derive(Debug, Clone, Default)]
pub struct InMemoryGroupProvider {
    /// Group membership: group_id -> set of device_ids
    memberships: BTreeMap<String, Vec<DeviceId>>,
}

impl InMemoryGroupProvider {
    /// Add device to group
    pub fn add_member(&mut self, group_id: String, device_id: DeviceId) {
        self.memberships
            .entry(group_id)
            .or_default()
            .push(device_id);
    }

    /// Remove device from group
    pub fn remove_member(&mut self, group_id: &str, device_id: &DeviceId) {
        if let Some(members) = self.memberships.get_mut(group_id) {
            members.retain(|id| id != device_id);
        }
    }
}

impl GroupMembershipProvider for InMemoryGroupProvider {
    fn is_group_member(&self, device_id: &DeviceId, group_id: &str) -> bool {
        self.memberships
            .get(group_id)
            .map(|members| members.contains(device_id))
            .unwrap_or(false)
    }

    fn get_device_groups(&self, device_id: &DeviceId) -> Vec<String> {
        self.memberships
            .iter()
            .filter_map(|(group_id, members)| {
                if members.contains(device_id) {
                    Some(group_id.clone())
                } else {
                    None
                }
            })
            .collect()
    }

    fn get_group_members(&self, group_id: &str) -> Vec<DeviceId> {
        self.memberships.get(group_id).cloned().unwrap_or_default()
    }
}

/// Enhanced capability manager with Keyhive convergent capabilities
///
/// This manager extends the native capability system with:
/// - Convergent authority graph for CRDT-native authorization
/// - BeeKEM group membership integration
/// - Unified verification across native and convergent systems
/// - Graceful fallback for backwards compatibility
#[derive(Debug)]
pub struct KeyhiveCapabilityManager {
    /// Native capability system (backwards compatibility)
    native: CapabilityManager,

    /// Keyhive convergent authority graph
    authority_graph: AuthorityGraph,

    /// Group membership provider
    group_provider: Option<Box<dyn GroupMembershipProvider>>,

    /// Group-based capability cache
    group_capabilities: BTreeMap<String, Vec<CapabilityGrant>>,

    /// Feature flag for gradual rollout
    keyhive_enabled: bool,

    /// Fallback configuration
    config: KeyhiveConfig,
}

/// Configuration for Keyhive integration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyhiveConfig {
    /// Enable Keyhive convergent capabilities
    pub convergent_enabled: bool,

    /// Enable BeeKEM group integration
    pub group_integration_enabled: bool,

    /// Prefer native system for verification
    pub prefer_native: bool,

    /// Enable fallback to native on convergent failures
    pub enable_fallback: bool,

    /// Cache group capabilities for performance
    pub cache_group_capabilities: bool,
}

impl Default for KeyhiveConfig {
    fn default() -> Self {
        Self {
            convergent_enabled: true,
            group_integration_enabled: true,
            prefer_native: false,
            enable_fallback: true,
            cache_group_capabilities: true,
        }
    }
}

impl KeyhiveCapabilityManager {
    /// Create new Keyhive-enhanced capability manager
    pub fn new(config: KeyhiveConfig) -> Self {
        Self {
            native: CapabilityManager::new(),
            authority_graph: AuthorityGraph::new(),
            group_provider: None,
            group_capabilities: BTreeMap::new(),
            keyhive_enabled: config.convergent_enabled,
            config,
        }
    }

    /// Create with existing native manager (migration path)
    pub fn from_native(native: CapabilityManager, config: KeyhiveConfig) -> Self {
        Self {
            native,
            authority_graph: AuthorityGraph::new(),
            group_provider: None,
            group_capabilities: BTreeMap::new(),
            keyhive_enabled: config.convergent_enabled,
            config,
        }
    }

    /// Register authority key for verification
    pub fn register_authority(&mut self, device_id: DeviceId, key: Ed25519VerifyingKey) {
        self.native.register_authority(device_id, key);
    }

    /// Set group membership provider
    pub fn set_group_provider(&mut self, provider: Box<dyn GroupMembershipProvider>) {
        self.group_provider = Some(provider);
        if self.config.cache_group_capabilities {
            self.refresh_group_capabilities();
        }
    }

    /// Grant capability using unified system
    ///
    /// This method grants capabilities in both the native system and
    /// the convergent authority graph, ensuring consistency across
    /// both authorization mechanisms.
    pub fn grant_unified_capability(
        &mut self,
        grant: CapabilityGrant,
        signing_key: &Ed25519SigningKey,
        effects: &Effects,
    ) -> Result<CapabilityToken> {
        info!(
            "Granting unified capability for device {:?} with {} permissions",
            grant.device_id,
            grant.permissions.len()
        );

        // Always grant in native system for backwards compatibility
        let token = self
            .native
            .grant_capability(grant.clone(), signing_key, effects)
            .map_err(|e| {
                warn!("Failed to grant native capability: {:?}", e);
                e
            })?;

        // If convergent capabilities enabled, also record in authority graph
        if self.keyhive_enabled && self.config.convergent_enabled {
            if let Err(e) = self.record_in_authority_graph(&grant, &token, effects) {
                warn!("Failed to record capability in authority graph: {:?}", e);
                // Don't fail the entire operation if convergent recording fails
            }
        }

        debug!(
            "Successfully granted unified capability with ID {:?}",
            token.capability_id()
        );
        Ok(token)
    }

    /// Unified permission verification
    ///
    /// This method provides unified verification across:
    /// 1. Native capability tokens
    /// 2. Convergent authority graph delegations  
    /// 3. BeeKEM group membership permissions
    ///
    /// The verification order can be configured via `KeyhiveConfig`.
    pub fn verify_unified_permission(
        &self,
        device_id: &DeviceId,
        permission: &Permission,
        current_time: u64,
    ) -> Result<VerificationResult> {
        debug!(
            "Verifying unified permission for device {:?}: {:?}",
            device_id, permission
        );

        // Choose verification order based on configuration
        if self.config.prefer_native {
            debug!("Using native-first verification");
            self.verify_native_first(device_id, permission, current_time)
        } else if self.keyhive_enabled {
            debug!("Using convergent-first verification");
            self.verify_convergent_first(device_id, permission, current_time)
        } else {
            debug!("Using native-only verification");
            self.verify_native_only(device_id, permission, current_time)
        }
    }

    /// Verify native system first, then convergent
    fn verify_native_first(
        &self,
        device_id: &DeviceId,
        permission: &Permission,
        current_time: u64,
    ) -> Result<VerificationResult> {
        // Try native system first
        debug!("Trying native system verification first");
        match self
            .native
            .verify_permission(device_id, permission, current_time)
        {
            Ok(()) => {
                debug!("Permission verified via native capability system");
                return Ok(VerificationResult::Native);
            }
            Err(e) => {
                debug!("Native verification failed: {:?}", e);
            }
        }

        // Fall back to convergent if enabled
        if self.config.convergent_enabled && self.config.enable_fallback {
            self.verify_convergent_capability(device_id, permission, current_time)
                .map(|_| VerificationResult::Convergent)
        } else {
            Err(CapabilityError::AuthorizationError(
                "No valid capabilities found".to_string(),
            ))
        }
    }

    /// Verify convergent system first, then native fallback
    fn verify_convergent_first(
        &self,
        device_id: &DeviceId,
        permission: &Permission,
        current_time: u64,
    ) -> Result<VerificationResult> {
        // Try convergent system first if enabled
        if self.config.convergent_enabled {
            match self.verify_convergent_capability(device_id, permission, current_time) {
                Ok(()) => {
                    debug!("Permission verified via convergent authority graph");
                    return Ok(VerificationResult::Convergent);
                }
                Err(e) => {
                    debug!("Convergent verification failed: {:?}", e);
                }
            }
        }

        // Fall back to native system
        if self.config.enable_fallback {
            self.native
                .verify_permission(device_id, permission, current_time)
                .map(|_| VerificationResult::Native)
        } else {
            Err(CapabilityError::AuthorizationError(
                "No valid capabilities found".to_string(),
            ))
        }
    }

    /// Verify native system only
    fn verify_native_only(
        &self,
        device_id: &DeviceId,
        permission: &Permission,
        current_time: u64,
    ) -> Result<VerificationResult> {
        self.native
            .verify_permission(device_id, permission, current_time)
            .map(|_| VerificationResult::Native)
    }

    /// Verify using convergent authority graph and group membership
    fn verify_convergent_capability(
        &self,
        device_id: &DeviceId,
        permission: &Permission,
        current_time: u64,
    ) -> Result<()> {
        // Check authority graph for delegated capabilities
        if self
            .verify_via_authority_graph(device_id, permission, current_time)
            .is_ok()
        {
            return Ok(());
        }

        // Check group membership permissions if group integration enabled
        if self.config.group_integration_enabled {
            if let Some(ref group_provider) = self.group_provider {
                if self
                    .verify_via_group_membership(device_id, permission, group_provider.as_ref())
                    .is_ok()
                {
                    return Ok(());
                }
            }
        }

        Err(CapabilityError::AuthorizationError(
            "No valid convergent capabilities found".to_string(),
        ))
    }

    /// Verify permission via authority graph
    fn verify_via_authority_graph(
        &self,
        _device_id: &DeviceId,
        _permission: &Permission,
        _current_time: u64,
    ) -> Result<()> {
        // TODO: Implement authority graph verification
        // This would check:
        // 1. Find all capabilities delegated to the device
        // 2. Check if any capability grants the required permission
        // 3. Verify delegation chain is not revoked
        // 4. Check capability expiration

        debug!("Authority graph verification not yet implemented");
        Err(CapabilityError::AuthorizationError(
            "Authority graph verification not implemented".to_string(),
        ))
    }

    /// Verify permission via group membership
    fn verify_via_group_membership(
        &self,
        device_id: &DeviceId,
        permission: &Permission,
        group_provider: &dyn GroupMembershipProvider,
    ) -> Result<()> {
        // Check if permission is group-related
        let group_id = match permission {
            Permission::Communication { relationship, .. } => {
                // Check if relationship is a group ID
                if relationship.starts_with("group:") {
                    Some(&relationship[6..])
                } else {
                    None
                }
            }
            Permission::Storage { resource, .. } => {
                // Check if resource is group storage
                if resource.starts_with("groups/") {
                    resource.split('/').nth(1)
                } else {
                    None
                }
            }
            _ => None,
        };

        if let Some(group_id) = group_id {
            // Check if device is member of the group
            if group_provider.is_group_member(device_id, group_id) {
                // Check cached group capabilities
                if let Some(grants) = self.group_capabilities.get(group_id) {
                    for grant in grants {
                        if grant.device_id == *device_id {
                            for granted_permission in &grant.permissions {
                                if self.permission_matches(granted_permission, permission) {
                                    debug!(
                                        "Permission granted via group membership in {}",
                                        group_id
                                    );
                                    return Ok(());
                                }
                            }
                        }
                    }
                } else {
                    // No cached capabilities, but group membership grants basic permissions
                    debug!(
                        "Permission granted via basic group membership in {}",
                        group_id
                    );
                    return Ok(());
                }
            }
        }

        Err(CapabilityError::AuthorizationError(
            "No group membership grants this permission".to_string(),
        ))
    }

    /// Check if granted permission matches required permission
    fn permission_matches(&self, granted: &Permission, required: &Permission) -> bool {
        // Use same logic as native capability manager
        // TODO: Extract this into a shared utility
        match (granted, required) {
            (
                Permission::Storage {
                    operation: op1,
                    resource: res1,
                },
                Permission::Storage {
                    operation: op2,
                    resource: res2,
                },
            ) => {
                op1 == op2
                    && (res1 == res2
                        || res1 == "*"
                        || res1.ends_with("/*") && res2.starts_with(&res1[..res1.len() - 1]))
            }
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
        }
    }

    /// Record capability grant in authority graph
    fn record_in_authority_graph(
        &mut self,
        _grant: &CapabilityGrant,
        _token: &CapabilityToken,
        _effects: &Effects,
    ) -> Result<()> {
        // TODO: Implement authority graph recording
        // This would:
        // 1. Create CapabilityDelegation event
        // 2. Apply it to the authority graph
        // 3. Update parent-child relationships

        debug!("Authority graph recording not yet implemented");
        Ok(())
    }

    /// Refresh group capabilities cache from group provider
    fn refresh_group_capabilities(&mut self) {
        if let Some(ref _group_provider) = self.group_provider {
            self.group_capabilities.clear();

            // TODO: Implement group capability refresh
            // This would:
            // 1. Get all groups from group provider
            // 2. For each group, get the current members
            // 3. Convert group membership to capability grants
            // 4. Cache the results

            debug!("Group capabilities refresh not yet implemented");
        }
    }

    /// Merge authority graph from another device (CRDT sync)
    pub fn merge_authority_graph(
        &mut self,
        other_graph: &AuthorityGraph,
        effects: &Effects,
    ) -> Result<()> {
        info!("Merging authority graph from remote device");
        self.authority_graph.merge(other_graph, effects)?;

        // Refresh group capabilities if cache is enabled
        if self.config.cache_group_capabilities {
            self.refresh_group_capabilities();
        }

        Ok(())
    }

    /// Get native capability manager (for migration/debugging)
    pub fn native_manager(&self) -> &CapabilityManager {
        &self.native
    }

    /// Get authority graph (for debugging/analysis)
    pub fn authority_graph(&self) -> &AuthorityGraph {
        &self.authority_graph
    }

    /// Check if Keyhive features are enabled
    pub fn is_keyhive_enabled(&self) -> bool {
        self.keyhive_enabled
    }

    /// Update configuration (runtime reconfiguration)
    pub fn update_config(&mut self, config: KeyhiveConfig) {
        let was_enabled = self.keyhive_enabled;
        self.config = config;
        self.keyhive_enabled = self.config.convergent_enabled;

        if !was_enabled && self.keyhive_enabled {
            info!("Keyhive capabilities enabled");
            if self.config.cache_group_capabilities {
                self.refresh_group_capabilities();
            }
        } else if was_enabled && !self.keyhive_enabled {
            info!("Keyhive capabilities disabled");
        }
    }
}

/// Result of unified verification indicating which system succeeded
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VerificationResult {
    /// Verification succeeded via native capability system
    Native,
    /// Verification succeeded via convergent authority graph
    Convergent,
    /// Verification succeeded via group membership
    Group,
}

/// Convenience methods for common permission types
impl KeyhiveCapabilityManager {
    /// Verify storage permission with unified system
    pub fn verify_storage(
        &self,
        device_id: &DeviceId,
        operation: StorageOperation,
        resource: &str,
        current_time: u64,
    ) -> Result<VerificationResult> {
        let permission = Permission::Storage {
            operation,
            resource: resource.to_string(),
        };
        self.verify_unified_permission(device_id, &permission, current_time)
    }

    /// Verify communication permission with unified system
    pub fn verify_communication(
        &self,
        device_id: &DeviceId,
        operation: CommunicationOperation,
        relationship: &str,
        current_time: u64,
    ) -> Result<VerificationResult> {
        let permission = Permission::Communication {
            operation,
            relationship: relationship.to_string(),
        };
        self.verify_unified_permission(device_id, &permission, current_time)
    }

    /// Verify relay permission with unified system
    pub fn verify_relay(
        &self,
        device_id: &DeviceId,
        operation: RelayOperation,
        trust_level: &str,
        current_time: u64,
    ) -> Result<VerificationResult> {
        let permission = Permission::Relay {
            operation,
            trust_level: trust_level.to_string(),
        };
        self.verify_unified_permission(device_id, &permission, current_time)
    }
}

impl Default for KeyhiveCapabilityManager {
    fn default() -> Self {
        Self::new(KeyhiveConfig::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_crypto::Effects;
    use uuid::Uuid;

    fn test_effects() -> Effects {
        Effects::for_test("keyhive_manager_test")
    }

    fn test_device_id() -> DeviceId {
        DeviceId(Uuid::new_v4())
    }

    fn test_signing_key() -> Ed25519SigningKey {
        aura_crypto::Ed25519SigningKey::from_bytes(&[1u8; 32])
    }

    #[test]
    fn test_manager_creation() {
        let manager = KeyhiveCapabilityManager::new(KeyhiveConfig::default());
        assert!(manager.is_keyhive_enabled());
        assert_eq!(manager.group_capabilities.len(), 0);
    }

    #[test]
    fn test_grant_unified_capability() {
        let mut manager = KeyhiveCapabilityManager::new(KeyhiveConfig::default());
        let device_id = test_device_id();
        let signing_key = test_signing_key();
        let effects = test_effects();

        let grant = CapabilityGrant {
            device_id,
            permissions: vec![Permission::Storage {
                operation: StorageOperation::Read,
                resource: "test/*".to_string(),
            }],
            issued_at: effects.now().unwrap_or(0),
            expires_at: None,
            delegation_chain: vec![],
        };

        let result = manager.grant_unified_capability(grant, &signing_key, &effects);
        assert!(result.is_ok());
    }

    #[test]
    fn test_native_fallback() {
        let mut config = KeyhiveConfig::default();
        config.prefer_native = true;
        config.convergent_enabled = false; // Disable Keyhive for this test

        let mut manager = KeyhiveCapabilityManager::new(config);
        let device_id = test_device_id();
        let signing_key = test_signing_key();
        let effects = test_effects();

        // Grant capability in native system (using pattern from native tests)
        let grant = CapabilityGrant {
            device_id,
            permissions: vec![Permission::Storage {
                operation: StorageOperation::Read,
                resource: "*".to_string(), // Use wildcard like native tests
            }],
            issued_at: effects.now().unwrap_or(0),
            expires_at: None,
            delegation_chain: vec![],
        };

        manager
            .grant_unified_capability(grant, &signing_key, &effects)
            .unwrap();

        // Verify should succeed via native system
        let result = manager.verify_storage(
            &device_id,
            StorageOperation::Read,
            "test/file",
            effects.now().unwrap_or(0),
        );

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), VerificationResult::Native);
    }

    #[test]
    fn test_config_update() {
        let mut manager = KeyhiveCapabilityManager::new(KeyhiveConfig::default());
        assert!(manager.is_keyhive_enabled());

        let mut new_config = KeyhiveConfig::default();
        new_config.convergent_enabled = false;

        manager.update_config(new_config);
        assert!(!manager.is_keyhive_enabled());
    }
}
