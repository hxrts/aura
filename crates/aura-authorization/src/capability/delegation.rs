//! Capability delegation system

use super::token::{CapabilityCondition, CapabilityToken};
use crate::{Action, AuthorizationError, Result, Subject};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A capability delegation record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityDelegation {
    /// Unique identifier for this delegation
    pub delegation_id: Uuid,

    /// The original capability being delegated
    pub parent_capability_id: Uuid,

    /// Who is delegating the capability
    pub delegator: Subject,

    /// Who is receiving the delegated capability
    pub delegatee: Subject,

    /// The new capability token created by delegation
    pub delegated_capability: CapabilityToken,

    /// When this delegation was created
    pub delegated_at: u64,

    /// Signature from the delegator proving they authorized this delegation
    pub delegator_signature: aura_crypto::Ed25519Signature,

    /// Optional restrictions imposed by the delegator
    pub restrictions: Vec<DelegationRestriction>,
}

/// Restrictions that can be imposed during delegation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DelegationRestriction {
    /// Reduce the set of allowed actions
    ReduceActions { allowed_actions: Vec<Action> },

    /// Add time restrictions
    TimeRestriction { expires_at: u64 },

    /// Prevent further delegation
    NonDelegatable,

    /// Reduce delegation depth
    ReduceDepth { max_depth: u8 },

    /// Add usage limits
    UsageLimit { max_uses: u32 },
}

/// Delegate a capability to another subject
///
/// This creates a new capability token for the delegatee with potentially
/// reduced permissions, based on the delegator's capability and any restrictions.
pub fn delegate_capability(
    parent_capability: &CapabilityToken,
    delegator: Subject,
    delegatee: Subject,
    restrictions: Vec<DelegationRestriction>,
    _delegator_signing_key: &aura_crypto::Ed25519SigningKey,
) -> Result<CapabilityDelegation> {
    // Verify the parent capability can be delegated
    if !parent_capability.delegatable {
        return Err(AuthorizationError::InvalidDelegationChain(
            "Parent capability is not delegatable".to_string(),
        ));
    }

    // Verify delegation depth
    if parent_capability.delegation_depth == 0 {
        return Err(AuthorizationError::InvalidDelegationChain(
            "Maximum delegation depth reached".to_string(),
        ));
    }

    // Create the new delegated capability
    let mut delegated_capability =
        create_delegated_capability(parent_capability, &delegatee, &restrictions)?;

    // Sign the delegated capability
    // Note: In a real implementation, this would be signed by the appropriate key
    // For now, we use a placeholder signature
    delegated_capability.issuer_signature = aura_crypto::Ed25519Signature::default();

    // Create the delegation record
    let delegation_id = Uuid::new_v4();
    let current_time = current_timestamp();

    let delegation = CapabilityDelegation {
        delegation_id,
        parent_capability_id: parent_capability.id,
        delegator,
        delegatee,
        delegated_capability,
        delegated_at: current_time,
        delegator_signature: aura_crypto::Ed25519Signature::default(), // Placeholder
        restrictions,
    };

    Ok(delegation)
}

/// Create a new capability based on the parent and restrictions
fn create_delegated_capability(
    parent: &CapabilityToken,
    delegatee: &Subject,
    restrictions: &[DelegationRestriction],
) -> Result<CapabilityToken> {
    let mut new_capability = CapabilityToken::new(
        delegatee.clone(),
        parent.resource.clone(),
        parent.actions.clone(),
        parent.issuer, // Keep original issuer
        parent.delegatable,
        parent.delegation_depth.saturating_sub(1),
    );

    // Copy expiration from parent if it exists
    new_capability.expires_at = parent.expires_at;

    // Apply restrictions
    for restriction in restrictions {
        apply_restriction(&mut new_capability, restriction)?;
    }

    Ok(new_capability)
}

/// Apply a delegation restriction to a capability
fn apply_restriction(
    capability: &mut CapabilityToken,
    restriction: &DelegationRestriction,
) -> Result<()> {
    match restriction {
        DelegationRestriction::ReduceActions { allowed_actions } => {
            // Keep only actions that are in both the original and allowed sets
            capability
                .actions
                .retain(|action| allowed_actions.contains(action));
        }

        DelegationRestriction::TimeRestriction { expires_at } => {
            // Set expiration to the more restrictive of parent and restriction
            match capability.expires_at {
                Some(current_expiry) => {
                    capability.expires_at = Some(current_expiry.min(*expires_at));
                }
                None => {
                    capability.expires_at = Some(*expires_at);
                }
            }
        }

        DelegationRestriction::NonDelegatable => {
            capability.delegatable = false;
        }

        DelegationRestriction::ReduceDepth { max_depth } => {
            capability.delegation_depth = capability.delegation_depth.min(*max_depth);
        }

        DelegationRestriction::UsageLimit { max_uses } => {
            capability.add_condition(CapabilityCondition::UsageLimit {
                max_uses: *max_uses,
                current_uses: 0,
            });
        }
    }

    Ok(())
}

/// Verify that a delegation is valid
pub fn verify_delegation(
    delegation: &CapabilityDelegation,
    parent_capability: &CapabilityToken,
    _delegator_public_key: &aura_crypto::Ed25519VerifyingKey,
) -> Result<()> {
    // Verify parent capability ID matches
    if delegation.parent_capability_id != parent_capability.id {
        return Err(AuthorizationError::InvalidDelegationChain(
            "Parent capability ID mismatch".to_string(),
        ));
    }

    // Verify parent capability was delegatable
    if !parent_capability.delegatable {
        return Err(AuthorizationError::InvalidDelegationChain(
            "Parent capability was not delegatable".to_string(),
        ));
    }

    // Verify delegated capability has reduced or equal permissions
    verify_capability_reduction(parent_capability, &delegation.delegated_capability)?;

    // Verify delegator signature (placeholder for now)
    // In a real implementation, this would verify the actual signature

    Ok(())
}

/// Verify that a delegated capability has reduced or equal permissions to parent
fn verify_capability_reduction(
    parent: &CapabilityToken,
    delegated: &CapabilityToken,
) -> Result<()> {
    // Check that all actions in delegated are also in parent
    for action in &delegated.actions {
        if !parent.actions.contains(action) {
            return Err(AuthorizationError::InvalidDelegationChain(format!(
                "Delegated capability has action {:?} not in parent",
                action
            )));
        }
    }

    // Check that delegation depth is reduced
    if delegated.delegation_depth >= parent.delegation_depth {
        return Err(AuthorizationError::InvalidDelegationChain(
            "Delegated capability must have reduced delegation depth".to_string(),
        ));
    }

    // Check that expiration is not extended
    match (parent.expires_at, delegated.expires_at) {
        (Some(parent_expiry), Some(delegated_expiry)) => {
            if delegated_expiry > parent_expiry {
                return Err(AuthorizationError::InvalidDelegationChain(
                    "Delegated capability cannot have later expiration than parent".to_string(),
                ));
            }
        }
        (Some(_), None) => {
            return Err(AuthorizationError::InvalidDelegationChain(
                "Delegated capability cannot be non-expiring if parent expires".to_string(),
            ));
        }
        _ => {
            // Other cases are allowed
        }
    }

    Ok(())
}

/// Get current timestamp (placeholder implementation)
fn current_timestamp() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Action, Resource, Subject};
    use aura_crypto::Effects;
    use aura_types::{AccountIdExt, DeviceIdExt};

    fn create_test_capability(effects: &Effects) -> CapabilityToken {
        let subject = Subject::Device(aura_types::DeviceId::new_with_effects(effects));
        let resource = Resource::Account(aura_types::AccountId::new_with_effects(effects));
        let actions = vec![Action::Read, Action::Write, Action::Delete];
        let issuer = aura_types::DeviceId::new_with_effects(effects);

        CapabilityToken::new(subject, resource, actions, issuer, true, 3)
    }

    #[test]
    fn test_successful_delegation() {
        let effects = Effects::test();
        let parent_capability = create_test_capability(&effects);

        let delegator = Subject::Device(aura_types::DeviceId::new_with_effects(&effects));
        let delegatee = Subject::Device(aura_types::DeviceId::new_with_effects(&effects));

        let restrictions = vec![
            DelegationRestriction::ReduceActions {
                allowed_actions: vec![Action::Read, Action::Write],
            },
            DelegationRestriction::NonDelegatable,
        ];

        let signing_key = aura_crypto::generate_ed25519_key();

        let delegation = delegate_capability(
            &parent_capability,
            delegator,
            delegatee,
            restrictions,
            &signing_key,
        );

        assert!(delegation.is_ok());
        let delegation = delegation.unwrap();

        // Check that restrictions were applied
        assert_eq!(delegation.delegated_capability.actions.len(), 2);
        assert!(!delegation.delegated_capability.delegatable);
        assert_eq!(delegation.delegated_capability.delegation_depth, 2);
    }

    #[test]
    fn test_delegation_non_delegatable_parent() {
        let effects = Effects::test();
        let mut parent_capability = create_test_capability(&effects);
        parent_capability.delegatable = false;

        let delegator = Subject::Device(aura_types::DeviceId::new_with_effects(&effects));
        let delegatee = Subject::Device(aura_types::DeviceId::new_with_effects(&effects));

        let signing_key = aura_crypto::generate_ed25519_key();

        let result = delegate_capability(
            &parent_capability,
            delegator,
            delegatee,
            vec![],
            &signing_key,
        );

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            AuthorizationError::InvalidDelegationChain(_)
        ));
    }

    #[test]
    fn test_delegation_max_depth_reached() {
        let effects = Effects::test();
        let mut parent_capability = create_test_capability(&effects);
        parent_capability.delegation_depth = 0;

        let delegator = Subject::Device(aura_types::DeviceId::new_with_effects(&effects));
        let delegatee = Subject::Device(aura_types::DeviceId::new_with_effects(&effects));

        let signing_key = aura_crypto::generate_ed25519_key();

        let result = delegate_capability(
            &parent_capability,
            delegator,
            delegatee,
            vec![],
            &signing_key,
        );

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            AuthorizationError::InvalidDelegationChain(_)
        ));
    }

    #[test]
    fn test_verify_capability_reduction() {
        let effects = Effects::test();
        let parent = create_test_capability(&effects);

        let mut valid_delegated = create_test_capability(&effects);
        valid_delegated.actions = vec![Action::Read]; // Subset of parent
        valid_delegated.delegation_depth = 2; // Less than parent

        assert!(verify_capability_reduction(&parent, &valid_delegated).is_ok());

        let mut invalid_delegated = create_test_capability(&effects);
        invalid_delegated.actions = vec![Action::Read, Action::Admin]; // Admin not in parent
        invalid_delegated.delegation_depth = 2;

        assert!(verify_capability_reduction(&parent, &invalid_delegated).is_err());
    }
}
