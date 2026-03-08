//! Scoped OTA activation and policy domains.

use aura_core::{AuthorityId, ContextId, DeviceId, HomeId, NeighborhoodId};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// Scope in which a release may be staged, activated, cut over, or rolled back.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum AuraActivationScope {
    /// Activation local to one concrete device.
    DeviceLocal {
        /// Device performing the activation.
        device_id: DeviceId,
    },
    /// Activation local to one authority runtime domain.
    AuthorityLocal {
        /// Authority owning the activation decision.
        authority_id: AuthorityId,
    },
    /// Activation coordinated inside one relational context.
    RelationalContext {
        /// Context governing the scoped activation.
        context_id: ContextId,
    },
    /// Activation scoped to one home in Aura's social topology.
    Home {
        /// Home whose local policy governs activation.
        home_id: HomeId,
    },
    /// Activation scoped to one neighborhood in Aura's social topology.
    Neighborhood {
        /// Neighborhood whose shared policy governs activation.
        neighborhood_id: NeighborhoodId,
    },
    /// Activation coordinated by an explicitly enumerated quorum.
    ManagedQuorum {
        /// Context binding the managed quorum.
        context_id: ContextId,
        /// Authorities participating in the quorum.
        participants: BTreeSet<AuthorityId>,
    },
}

impl AuraActivationScope {
    /// Policy scope that governs activation decisions for this activation scope.
    pub fn policy_scope(&self) -> AuraPolicyScope {
        match self {
            Self::DeviceLocal { device_id } => AuraPolicyScope::Device {
                device_id: *device_id,
            },
            Self::AuthorityLocal { authority_id } => AuraPolicyScope::Authority {
                authority_id: *authority_id,
            },
            Self::RelationalContext { context_id } => AuraPolicyScope::RelationalContext {
                context_id: *context_id,
            },
            Self::Home { home_id } => AuraPolicyScope::Home { home_id: *home_id },
            Self::Neighborhood { neighborhood_id } => AuraPolicyScope::Neighborhood {
                neighborhood_id: *neighborhood_id,
            },
            Self::ManagedQuorum { context_id, .. } => AuraPolicyScope::ManagedQuorum {
                context_id: *context_id,
            },
        }
    }
}

/// Scope at which OTA discovery, sharing, or activation policy is published.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum AuraPolicyScope {
    /// Policy published for one device.
    Device {
        /// Device governed by the policy.
        device_id: DeviceId,
    },
    /// Policy published for one authority.
    Authority {
        /// Authority governed by the policy.
        authority_id: AuthorityId,
    },
    /// Policy published for one relational context.
    RelationalContext {
        /// Context governed by the policy.
        context_id: ContextId,
    },
    /// Policy published for one home.
    Home {
        /// Home governed by the policy.
        home_id: HomeId,
    },
    /// Policy published for one neighborhood.
    Neighborhood {
        /// Neighborhood governed by the policy.
        neighborhood_id: NeighborhoodId,
    },
    /// Policy published for a managed quorum context.
    ManagedQuorum {
        /// Context binding the managed quorum policy.
        context_id: ContextId,
    },
}

/// Which release set may currently run inside a scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum ReleaseResidency {
    /// Only the currently active legacy release may admit new work.
    LegacyOnly,
    /// Legacy and target releases may both run under explicit coexistence rules.
    Coexisting,
    /// Only the target release may admit new work.
    TargetOnly,
}

/// Transition activity currently in flight for a scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum TransitionState {
    /// No cutover or rollback transition is currently executing.
    Idle,
    /// The scope is staged and gathering the remaining cutover evidence.
    AwaitingCutover,
    /// The scope is actively switching to the target release.
    CuttingOver,
    /// The scope is reverting to the prior release.
    RollingBack,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn activation_scopes_map_to_policy_scopes() {
        let device = AuraActivationScope::DeviceLocal {
            device_id: DeviceId::new_from_entropy([1; 32]),
        };
        assert_eq!(
            device.policy_scope(),
            AuraPolicyScope::Device {
                device_id: DeviceId::new_from_entropy([1; 32]),
            }
        );

        let authority = AuraActivationScope::AuthorityLocal {
            authority_id: AuthorityId::new_from_entropy([2; 32]),
        };
        assert_eq!(
            authority.policy_scope(),
            AuraPolicyScope::Authority {
                authority_id: AuthorityId::new_from_entropy([2; 32]),
            }
        );

        let context_id = ContextId::new_from_entropy([3; 32]);
        let relational = AuraActivationScope::RelationalContext { context_id };
        assert_eq!(
            relational.policy_scope(),
            AuraPolicyScope::RelationalContext { context_id }
        );

        let home_id = HomeId::from_bytes([4; 32]);
        let home = AuraActivationScope::Home { home_id };
        assert_eq!(home.policy_scope(), AuraPolicyScope::Home { home_id });

        let neighborhood_id = NeighborhoodId::from_bytes([5; 32]);
        let neighborhood = AuraActivationScope::Neighborhood { neighborhood_id };
        assert_eq!(
            neighborhood.policy_scope(),
            AuraPolicyScope::Neighborhood { neighborhood_id }
        );

        let managed = AuraActivationScope::ManagedQuorum {
            context_id,
            participants: BTreeSet::from([
                AuthorityId::new_from_entropy([6; 32]),
                AuthorityId::new_from_entropy([7; 32]),
            ]),
        };
        assert_eq!(
            managed.policy_scope(),
            AuraPolicyScope::ManagedQuorum { context_id }
        );
    }
}
