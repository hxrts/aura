//! Capability delegation chains with proper attenuation

use crate::{CapabilitySet, WotError};
use aura_core::identifiers::DeviceId;
use serde::{Deserialize, Serialize};
use std::time::SystemTime;

/// A single link in a delegation chain
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DelegationLink {
    /// The device that delegated the capability
    pub delegator: DeviceId,

    /// The device that received the delegation
    pub delegatee: DeviceId,

    /// The capabilities being delegated (must be subset of delegator's caps)
    pub capabilities: CapabilitySet,

    /// When this delegation was created
    pub created_at: SystemTime,

    /// When this delegation expires (if any)
    pub expires_at: Option<SystemTime>,

    /// Maximum depth for further delegation
    pub max_delegation_depth: u32,

    /// Cryptographic signature from delegator
    pub signature: Vec<u8>, // In real implementation, use proper signature type
}

impl DelegationLink {
    /// Create a new delegation link
    #[allow(clippy::disallowed_methods)]
    pub fn new(
        delegator: DeviceId,
        delegatee: DeviceId,
        capabilities: CapabilitySet,
        max_delegation_depth: u32,
    ) -> Self {
        Self {
            delegator,
            delegatee,
            capabilities,
            created_at: SystemTime::UNIX_EPOCH,
            expires_at: None,
            max_delegation_depth,
            signature: vec![], // Would be computed in real implementation
        }
    }

    /// Check if this delegation is currently valid
    #[allow(clippy::disallowed_methods)]
    pub fn is_valid(&self) -> bool {
        if let Some(expires_at) = self.expires_at {
            SystemTime::UNIX_EPOCH < expires_at
        } else {
            true
        }
    }

    /// Attenuate capabilities through this delegation link
    ///
    /// The result can only be equal to or more restrictive than the input
    pub fn attenuate(&self, input_capabilities: &CapabilitySet) -> CapabilitySet {
        // Delegation can only restrict, never expand capabilities
        input_capabilities.meet(&self.capabilities)
    }
}

/// A chain of capability delegations
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DelegationChain {
    /// The delegation links in order from root to final delegatee
    pub links: Vec<DelegationLink>,
}

impl DelegationChain {
    /// Create a new empty delegation chain
    pub fn new() -> Self {
        Self { links: Vec::new() }
    }

    /// Create a delegation chain from a single delegation
    pub fn from_delegation(link: DelegationLink) -> Self {
        Self { links: vec![link] }
    }

    /// Add a delegation link to the chain
    pub fn add_delegation(&mut self, link: DelegationLink) -> Result<(), WotError> {
        // Verify the delegation is valid
        if !link.is_valid() {
            return Err(WotError::invalid("Delegation has expired"));
        }

        // Check delegation depth - find the minimum allowed depth in the chain
        let mut min_allowed_depth = u32::MAX;
        for (i, existing_link) in self.links.iter().enumerate() {
            let remaining_depth = existing_link
                .max_delegation_depth
                .saturating_sub(i as u32 + 1);
            min_allowed_depth = min_allowed_depth.min(remaining_depth);
        }

        // If adding this link would exceed the minimum allowed depth, reject it
        if min_allowed_depth == 0 && !self.links.is_empty() {
            return Err(WotError::invalid(format!(
                "Delegation depth exceeded: attempted {} > max {}",
                self.links.len() + 1,
                self.links
                    .iter()
                    .map(|link| link.max_delegation_depth)
                    .min()
                    .unwrap_or(0)
            )));
        }

        // Verify chain continuity - new delegator must be previous delegatee
        if let Some(last_link) = self.links.last() {
            if last_link.delegatee != link.delegator {
                return Err(WotError::invalid(format!(
                    "Chain broken: last delegatee {} != new delegator {}",
                    last_link.delegatee, link.delegator
                )));
            }
        }

        self.links.push(link);
        Ok(())
    }

    /// Compute the effective capabilities at the end of this delegation chain
    ///
    /// This implements the meet-semilattice intersection across all delegations
    pub fn effective_capabilities(&self, root_capabilities: &CapabilitySet) -> CapabilitySet {
        let mut current_capabilities = root_capabilities.clone();

        // Apply each delegation link in sequence
        for link in &self.links {
            current_capabilities = link.attenuate(&current_capabilities);
        }

        current_capabilities
    }

    /// Get the final delegatee in the chain
    pub fn final_delegatee(&self) -> Option<DeviceId> {
        self.links.last().map(|link| link.delegatee)
    }

    /// Get the root delegator in the chain
    pub fn root_delegator(&self) -> Option<DeviceId> {
        self.links.first().map(|link| link.delegator)
    }

    /// Validate the entire delegation chain
    pub fn validate(&self) -> Result<(), WotError> {
        if self.links.is_empty() {
            return Ok(());
        }

        // Check each link is valid
        for link in &self.links {
            if !link.is_valid() {
                return Err(WotError::invalid(format!(
                    "Invalid delegation from {} to {}",
                    link.delegator, link.delegatee
                )));
            }
        }

        // Check chain continuity
        for window in self.links.windows(2) {
            if window[0].delegatee != window[1].delegator {
                return Err(WotError::invalid("Broken delegation chain continuity"));
            }
        }

        // Check delegation depths
        for (i, link) in self.links.iter().enumerate() {
            if i >= link.max_delegation_depth as usize {
                return Err(WotError::invalid(format!(
                    "Delegation depth exceeded: {} >= max {}",
                    i, link.max_delegation_depth
                )));
            }
        }

        Ok(())
    }

    /// Check if this delegation chain permits a specific operation
    pub fn permits_operation(&self, root_capabilities: &CapabilitySet, operation: &str) -> bool {
        let effective_caps = self.effective_capabilities(root_capabilities);
        effective_caps.permits(operation)
    }
}

impl Default for DelegationChain {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::identifiers::DeviceId;

    #[test]
    fn test_delegation_chain_attenuation() {
        let device_a = DeviceId::new();
        let device_b = DeviceId::new();
        let device_c = DeviceId::new();

        // Root capabilities: read and write
        let root_caps = CapabilitySet::from_permissions(&["read", "write"]);

        // First delegation: A delegates read+write to B, but limits to read only
        let delegation_ab = DelegationLink::new(
            device_a,
            device_b,
            CapabilitySet::from_permissions(&["read"]),
            2, // Allow one more delegation
        );

        // Second delegation: B delegates to C, but already limited to read
        let delegation_bc = DelegationLink::new(
            device_b,
            device_c,
            CapabilitySet::from_permissions(&["read", "write"]), // B tries to delegate write
            1,
        );

        let mut chain = DelegationChain::new();
        chain.add_delegation(delegation_ab).unwrap();
        chain.add_delegation(delegation_bc).unwrap();

        // Effective capabilities should be limited by the most restrictive delegation
        let effective = chain.effective_capabilities(&root_caps);
        assert!(effective.permits("read"));
        assert!(!effective.permits("write")); // Should be attenuated by first delegation
    }

    #[test]
    fn test_delegation_depth_enforcement() {
        let device_a = DeviceId::new();
        let device_b = DeviceId::new();
        let device_c = DeviceId::new();

        let delegation = DelegationLink::new(
            device_a,
            device_b,
            CapabilitySet::from_permissions(&["read"]),
            0, // No further delegation allowed
        );

        let mut chain = DelegationChain::new();
        chain.add_delegation(delegation).unwrap();

        // Attempting to add another delegation should fail
        let further_delegation = DelegationLink::new(
            device_b,
            device_c,
            CapabilitySet::from_permissions(&["read"]),
            0,
        );

        assert!(chain.add_delegation(further_delegation).is_err());
    }

    #[test]
    fn test_chain_continuity() {
        let device_a = DeviceId::new();
        let device_b = DeviceId::new();
        let device_c = DeviceId::new();
        let device_d = DeviceId::new();

        let delegation_ab = DelegationLink::new(
            device_a,
            device_b,
            CapabilitySet::from_permissions(&["read"]),
            2,
        );

        // Break continuity - C was not delegated to by B
        let delegation_cd = DelegationLink::new(
            device_c,
            device_d,
            CapabilitySet::from_permissions(&["read"]),
            1,
        );

        let mut chain = DelegationChain::new();
        chain.add_delegation(delegation_ab).unwrap();

        // This should fail due to broken continuity
        assert!(chain.add_delegation(delegation_cd).is_err());
    }
}
