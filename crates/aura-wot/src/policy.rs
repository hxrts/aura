//! Policy management for capability-based authorization

use crate::{CapabilitySet, WotError};
use aura_core::identifiers::DeviceId;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A policy defines the base capabilities for devices
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Policy {
    /// Device-specific capability assignments
    device_capabilities: HashMap<DeviceId, CapabilitySet>,

    /// Default capabilities for devices not explicitly listed
    default_capabilities: CapabilitySet,

    /// Policy metadata
    pub name: String,
    pub version: u32,
    pub created_at: std::time::SystemTime,
}

impl Policy {
    /// Create a new empty policy
    pub fn new() -> Self {
        Self {
            device_capabilities: HashMap::new(),
            default_capabilities: CapabilitySet::empty(),
            name: "default".to_string(),
            version: 1,
            created_at: std::time::SystemTime::now(),
        }
    }

    /// Create a policy with a name
    pub fn named(name: String) -> Self {
        Self {
            device_capabilities: HashMap::new(),
            default_capabilities: CapabilitySet::empty(),
            name,
            version: 1,
            created_at: std::time::SystemTime::now(),
        }
    }

    /// Set capabilities for a specific device
    pub fn set_device_capabilities(&mut self, device_id: DeviceId, capabilities: CapabilitySet) {
        self.device_capabilities.insert(device_id, capabilities);
    }

    /// Set default capabilities for devices not explicitly listed
    pub fn set_default_capabilities(&mut self, capabilities: CapabilitySet) {
        self.default_capabilities = capabilities;
    }

    /// Get capabilities for a specific device
    pub fn capabilities_for_device(&self, device_id: &DeviceId) -> CapabilitySet {
        self.device_capabilities
            .get(device_id)
            .cloned()
            .unwrap_or_else(|| self.default_capabilities.clone())
    }

    /// Remove device from policy
    pub fn remove_device(&mut self, device_id: &DeviceId) {
        self.device_capabilities.remove(device_id);
    }

    /// List all devices with explicit capability assignments
    pub fn devices(&self) -> impl Iterator<Item = &DeviceId> {
        self.device_capabilities.keys()
    }

    /// Check if a device has any capabilities
    pub fn device_has_capabilities(&self, device_id: &DeviceId) -> bool {
        let caps = self.capabilities_for_device(device_id);
        caps.capabilities().count() > 0
    }

    /// Merge two policies using meet-semilattice intersection
    ///
    /// The result contains the intersection of capabilities from both policies.
    /// This ensures that merging policies can only restrict, never expand capabilities.
    pub fn meet(&self, other: &Policy) -> Policy {
        let mut merged_capabilities = HashMap::new();

        // Get all devices from both policies
        let all_devices: std::collections::BTreeSet<DeviceId> = self
            .device_capabilities
            .keys()
            .chain(other.device_capabilities.keys())
            .copied()
            .collect();

        // Compute intersection for each device
        for device_id in all_devices {
            let caps1 = self.capabilities_for_device(&device_id);
            let caps2 = other.capabilities_for_device(&device_id);
            let merged_caps = caps1.meet(&caps2);

            merged_capabilities.insert(device_id, merged_caps);
        }

        Policy {
            device_capabilities: merged_capabilities,
            default_capabilities: self.default_capabilities.meet(&other.default_capabilities),
            name: format!("{}_meet_{}", self.name, other.name),
            version: std::cmp::max(self.version, other.version) + 1,
            created_at: std::time::SystemTime::now(),
        }
    }
}

impl Default for Policy {
    fn default() -> Self {
        Self::new()
    }
}

/// Policy engine for managing and evaluating policies
#[derive(Debug)]
pub struct PolicyEngine {
    /// Currently active policy
    active_policy: Policy,

    /// Policy history for audit purposes
    policy_history: Vec<Policy>,
}

impl PolicyEngine {
    /// Create a new policy engine with default policy
    pub fn new() -> Self {
        Self {
            active_policy: Policy::new(),
            policy_history: Vec::new(),
        }
    }

    /// Create a policy engine with an initial policy
    pub fn with_policy(policy: Policy) -> Self {
        Self {
            active_policy: policy,
            policy_history: Vec::new(),
        }
    }

    /// Update the active policy
    pub fn update_policy(&mut self, new_policy: Policy) {
        // Archive current policy
        let old_policy = std::mem::replace(&mut self.active_policy, new_policy);
        self.policy_history.push(old_policy);
    }

    /// Get the current active policy
    pub fn active_policy(&self) -> &Policy {
        &self.active_policy
    }

    /// Get policy history
    pub fn policy_history(&self) -> &[Policy] {
        &self.policy_history
    }

    /// Grant capabilities to a device
    pub fn grant_capabilities(&mut self, device_id: DeviceId, capabilities: CapabilitySet) {
        self.active_policy
            .set_device_capabilities(device_id, capabilities);
    }

    /// Revoke all capabilities from a device
    pub fn revoke_device(&mut self, device_id: &DeviceId) {
        self.active_policy.remove_device(device_id);
    }

    /// Merge another policy into the active policy using meet-semilattice
    ///
    /// This can only restrict capabilities, never expand them.
    pub fn merge_policy(&mut self, other: &Policy) {
        let merged = self.active_policy.meet(other);
        self.update_policy(merged);
    }

    /// Check if a device has permission for an operation
    pub fn check_permission(&self, device_id: &DeviceId, operation: &str) -> bool {
        let caps = self.active_policy.capabilities_for_device(device_id);
        caps.permits(operation)
    }

    /// Get effective capabilities for a device
    pub fn device_capabilities(&self, device_id: &DeviceId) -> CapabilitySet {
        self.active_policy.capabilities_for_device(device_id)
    }

    /// Validate policy consistency
    pub fn validate_policy(&self) -> Result<(), WotError> {
        // Check that all capability sets are valid
        for (device_id, caps) in &self.active_policy.device_capabilities {
            if caps.capabilities().count() == 0 {
                tracing::warn!("Device {} has empty capability set", device_id);
            }
        }

        Ok(())
    }
}

impl Default for PolicyEngine {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::CapabilitySet;

    #[test]
    fn test_policy_meet_operation() {
        let device_a = DeviceId::new();
        let device_b = DeviceId::new();

        // Policy 1: Device A has read+write, Device B has read
        let mut policy1 = Policy::named("policy1".to_string());
        policy1.set_device_capabilities(
            device_a,
            CapabilitySet::from_permissions(&["read", "write"]),
        );
        policy1.set_device_capabilities(device_b, CapabilitySet::from_permissions(&["read"]));

        // Policy 2: Device A has read, Device B has read+execute
        let mut policy2 = Policy::named("policy2".to_string());
        policy2.set_device_capabilities(device_a, CapabilitySet::from_permissions(&["read"]));
        policy2.set_device_capabilities(
            device_b,
            CapabilitySet::from_permissions(&["read", "execute:test"]),
        );

        // Meet operation should result in intersection
        let merged = policy1.meet(&policy2);

        // Device A: {read, write} ⊓ {read} = {read}
        let caps_a = merged.capabilities_for_device(&device_a);
        assert!(caps_a.permits("read"));
        assert!(!caps_a.permits("write"));

        // Device B: {read} ⊓ {read, execute} = {read}
        let caps_b = merged.capabilities_for_device(&device_b);
        assert!(caps_b.permits("read"));
        assert!(!caps_b.permits("execute:test"));
    }

    #[test]
    fn test_policy_engine_operations() {
        let mut engine = PolicyEngine::new();
        let device_id = DeviceId::new();

        // Initially no capabilities
        assert!(!engine.check_permission(&device_id, "read"));

        // Grant capabilities
        let caps = CapabilitySet::from_permissions(&["read", "write"]);
        engine.grant_capabilities(device_id, caps);

        assert!(engine.check_permission(&device_id, "read"));
        assert!(engine.check_permission(&device_id, "write"));

        // Revoke capabilities
        engine.revoke_device(&device_id);
        assert!(!engine.check_permission(&device_id, "read"));
    }

    #[test]
    fn test_policy_merge_restrictions() {
        let mut engine = PolicyEngine::new();
        let device_id = DeviceId::new();

        // Start with broad capabilities
        let initial_caps = CapabilitySet::from_permissions(&["read", "write", "execute:test"]);
        engine.grant_capabilities(device_id, initial_caps);

        // Merge with restrictive policy
        let mut restrictive_policy = Policy::named("restrictive".to_string());
        restrictive_policy
            .set_device_capabilities(device_id, CapabilitySet::from_permissions(&["read"]));

        engine.merge_policy(&restrictive_policy);

        // Should now only have read capability
        assert!(engine.check_permission(&device_id, "read"));
        assert!(!engine.check_permission(&device_id, "write"));
        assert!(!engine.check_permission(&device_id, "execute:test"));
    }
}
