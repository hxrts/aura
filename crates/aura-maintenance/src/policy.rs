//! Pure OTA discovery, sharing, and activation policy types.

use crate::{AuraActivationScope, AuraPolicyScope};
use aura_core::{AuthorityId, ContextId, HomeId, NeighborhoodId};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// Authority selector used by OTA policies.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AuthoritySelector {
    /// Match any authority.
    Any,
    /// Match one of the listed authorities.
    Exact(BTreeSet<AuthorityId>),
}

impl AuthoritySelector {
    /// Check whether the selector matches one authority id.
    pub fn matches(&self, authority_id: &AuthorityId) -> bool {
        match self {
            Self::Any => true,
            Self::Exact(allowed) => allowed.contains(authority_id),
        }
    }
}

/// Context selector used by OTA policies.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ContextSelector {
    /// Match any policy scope.
    Any,
    /// Match one of the listed relational contexts.
    Relational(BTreeSet<ContextId>),
    /// Match one of the listed homes.
    Home(BTreeSet<HomeId>),
    /// Match one of the listed neighborhoods.
    Neighborhood(BTreeSet<NeighborhoodId>),
}

impl ContextSelector {
    /// Check whether the selector matches a policy scope.
    pub fn matches_policy_scope(&self, scope: &AuraPolicyScope) -> bool {
        match (self, scope) {
            (Self::Any, _) => true,
            (Self::Relational(allowed), AuraPolicyScope::RelationalContext { context_id }) => {
                allowed.contains(context_id)
            }
            (Self::Home(allowed), AuraPolicyScope::Home { home_id }) => allowed.contains(home_id),
            (Self::Neighborhood(allowed), AuraPolicyScope::Neighborhood { neighborhood_id }) => {
                allowed.contains(neighborhood_id)
            }
            _ => false,
        }
    }
}

/// Pinning behavior for release-sharing policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PinPolicy {
    /// Do not pin release objects automatically.
    None,
    /// Pin manifests and certificates only.
    MetadataOnly,
    /// Pin manifests, certificates, and required artifacts.
    RequiredArtifacts,
    /// Pin the full release bundle.
    FullBundle,
}

/// Recommendation publication behavior for release-sharing policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RecommendationPolicy {
    /// Do not publish recommendations.
    Never,
    /// Publish recommendations only for explicitly trusted authorities.
    TrustedAuthoritiesOnly,
    /// Publish recommendations across the allowed scope.
    ScopeWide,
}

/// Discovery policy controls what release metadata a scope will accept.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuraReleaseDiscoveryPolicy {
    /// Allowed release authorities.
    pub allowed_release_sources: AuthoritySelector,
    /// Allowed builder authorities.
    pub allowed_builder_sources: AuthoritySelector,
    /// Allowed discovery scopes.
    pub allowed_contexts: ContextSelector,
    /// Whether manifests/certificates may be fetched automatically.
    pub auto_fetch_metadata: bool,
    /// Whether artifacts may be fetched automatically after discovery.
    pub auto_fetch_artifacts: bool,
}

/// Sharing policy controls redistribution of already-discovered release data.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuraReleaseSharingPolicy {
    /// Whether and how aggressively to pin release objects locally.
    pub pin_policy: PinPolicy,
    /// Whether manifests may be forwarded to other scopes.
    pub allow_manifest_forwarding: bool,
    /// Whether artifacts may be forwarded to other scopes.
    pub allow_artifact_forwarding: bool,
    /// Whether build certificates may be forwarded to other scopes.
    pub allow_certificate_forwarding: bool,
    /// Whether recommendations may be published from this scope.
    pub recommendation_policy: RecommendationPolicy,
}

/// Trust requirements for local staging or activation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuraActivationTrustPolicy {
    /// Allowed release authorities for activation.
    pub allowed_release_sources: AuthoritySelector,
    /// Allowed builder authorities for activation.
    pub allowed_builder_sources: AuthoritySelector,
    /// Minimum number of distinct builder certificates required, if any.
    pub required_builder_threshold: Option<u16>,
    /// Whether a local rebuild is required before activation.
    pub require_local_rebuild: bool,
}

/// Activation policy controls staging and activation inside one activation scope.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuraReleaseActivationPolicy {
    /// Trust requirements for staging and activation.
    pub trust_policy: AuraActivationTrustPolicy,
    /// Whether staged bundles may be prepared automatically.
    pub auto_stage: bool,
    /// Whether a fully trusted/staged bundle may activate automatically.
    pub auto_activate: bool,
    /// Whether local policy should honor the manifest's suggested activation time.
    ///
    /// When enabled, the suggestion is evaluated against the local wall clock and
    /// acts only as a "not before" hint. It is not a fence.
    pub respect_suggested_activation_time: bool,
    /// Activation scope governed by this policy.
    pub activation_scope: AuraActivationScope,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn authority_selector_matches_exact_sets() {
        let allowed = AuthoritySelector::Exact(BTreeSet::from([
            AuthorityId::new_from_entropy([1; 32]),
            AuthorityId::new_from_entropy([2; 32]),
        ]));

        assert!(allowed.matches(&AuthorityId::new_from_entropy([1; 32])));
        assert!(!allowed.matches(&AuthorityId::new_from_entropy([3; 32])));
    }

    #[test]
    fn context_selector_matches_policy_scope() {
        let context_id = ContextId::new_from_entropy([4; 32]);
        let selector = ContextSelector::Relational(BTreeSet::from([context_id]));
        assert!(selector.matches_policy_scope(&AuraPolicyScope::RelationalContext { context_id }));
        assert!(!selector.matches_policy_scope(&AuraPolicyScope::Authority {
            authority_id: AuthorityId::new_from_entropy([5; 32]),
        }));
    }
}
