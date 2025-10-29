//! Capability chain verification

use super::{CapabilityDelegation, CapabilityToken};
use crate::{AuthorizationError, Result};
use serde::{Deserialize, Serialize};

/// A chain of capability delegations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityChain {
    /// The root capability at the start of the chain
    pub root_capability: CapabilityToken,

    /// Ordered list of delegations in the chain
    pub delegations: Vec<CapabilityDelegation>,

    /// The final capability at the end of the chain
    pub final_capability: CapabilityToken,
}

impl CapabilityChain {
    /// Create a new capability chain starting with a root capability
    pub fn new(root_capability: CapabilityToken) -> Self {
        let final_capability = root_capability.clone();
        Self {
            root_capability,
            delegations: Vec::new(),
            final_capability,
        }
    }

    /// Add a delegation to the chain
    pub fn add_delegation(&mut self, delegation: CapabilityDelegation) {
        self.final_capability = delegation.delegated_capability.clone();
        self.delegations.push(delegation);
    }

    /// Get the length of the delegation chain
    pub fn chain_length(&self) -> usize {
        self.delegations.len()
    }
}

/// Verify that a capability chain is valid
pub fn verify_capability_chain(chain: &CapabilityChain) -> Result<()> {
    if chain.delegations.is_empty() {
        // No delegations, just verify root capability
        return Ok(());
    }

    // Verify each delegation in sequence
    let mut current_capability = &chain.root_capability;

    for delegation in &chain.delegations {
        // Verify this delegation is valid from the current capability
        if delegation.parent_capability_id != current_capability.id {
            return Err(AuthorizationError::InvalidDelegationChain(
                "Delegation parent ID mismatch".to_string(),
            ));
        }

        // Update current capability for next iteration
        current_capability = &delegation.delegated_capability;
    }

    // Verify final capability matches last delegation
    if let Some(last_delegation) = chain.delegations.last() {
        if chain.final_capability.id != last_delegation.delegated_capability.id {
            return Err(AuthorizationError::InvalidDelegationChain(
                "Final capability does not match last delegation".to_string(),
            ));
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Action, Resource, Subject};
    use aura_crypto::Effects;
    use aura_types::{AccountIdExt, DeviceIdExt};

    #[test]
    fn test_capability_chain_creation() {
        let effects = Effects::test();
        let subject = Subject::Device(aura_types::DeviceId::new_with_effects(&effects));
        let resource = Resource::Account(aura_types::AccountId::new_with_effects(&effects));
        let actions = vec![Action::Read, Action::Write];
        let issuer = aura_types::DeviceId::new_with_effects(&effects);

        let root_capability = CapabilityToken::new(subject, resource, actions, issuer, true, 3);
        let chain = CapabilityChain::new(root_capability.clone());

        assert_eq!(chain.root_capability.id, root_capability.id);
        assert_eq!(chain.final_capability.id, root_capability.id);
        assert_eq!(chain.chain_length(), 0);
    }

    #[test]
    fn test_verify_empty_chain() {
        let effects = Effects::test();
        let subject = Subject::Device(aura_types::DeviceId::new_with_effects(&effects));
        let resource = Resource::Account(aura_types::AccountId::new_with_effects(&effects));
        let actions = vec![Action::Read];
        let issuer = aura_types::DeviceId::new_with_effects(&effects);

        let root_capability = CapabilityToken::new(subject, resource, actions, issuer, true, 3);
        let chain = CapabilityChain::new(root_capability);

        assert!(verify_capability_chain(&chain).is_ok());
    }
}
