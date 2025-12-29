//! Relay Candidate Building
//!
//! Provides structured relay candidate building from social topology.

use crate::SocialTopology;
use aura_core::{
    effects::relay::{RelayCandidate, RelayContext},
    identifiers::AuthorityId,
};
use std::sync::Arc;

/// Trait for checking if peers are currently reachable.
///
/// Implementations might check:
/// - Active connections
/// - Recent heartbeat timestamps
/// - Network partition state
pub trait ReachabilityChecker: Send + Sync {
    /// Check if a peer is currently reachable.
    fn is_reachable(&self, peer: &AuthorityId) -> bool;
}

/// Blanket implementation for closures.
impl<F> ReachabilityChecker for F
where
    F: Fn(&AuthorityId) -> bool + Send + Sync,
{
    fn is_reachable(&self, peer: &AuthorityId) -> bool {
        (self)(peer)
    }
}

/// Builder for relay candidates based on social topology.
///
/// Wraps `SocialTopology` to provide a structured API for building
/// relay candidates using `RelayContext`.
///
/// # Example
///
/// ```ignore
/// let builder = RelayCandidateBuilder::new(topology);
/// let candidates = builder.build_candidates(&context, &reachability);
/// ```
pub struct RelayCandidateBuilder {
    /// The social topology to query.
    social: Arc<SocialTopology>,
}

impl RelayCandidateBuilder {
    /// Create a new relay candidate builder.
    pub fn new(social: Arc<SocialTopology>) -> Self {
        Self { social }
    }

    /// Create from a topology reference.
    pub fn from_topology(social: SocialTopology) -> Self {
        Self {
            social: Arc::new(social),
        }
    }

    /// Build relay candidates for a context.
    ///
    /// Returns candidates in priority order:
    /// 1. Home peers (highest priority)
    /// 2. Neighborhood peers
    /// 3. Guardians (fallback)
    ///
    /// # Arguments
    /// * `context` - The relay context with source, destination, etc.
    /// * `reachability` - Checker for peer reachability
    pub fn build_candidates(
        &self,
        context: &RelayContext,
        reachability: &impl ReachabilityChecker,
    ) -> Vec<RelayCandidate> {
        self.social
            .build_relay_candidates(&context.destination, |peer| reachability.is_reachable(peer))
    }

    /// Build candidates with specific requirements.
    ///
    /// Filters candidates based on additional criteria.
    ///
    /// # Arguments
    /// * `context` - The relay context
    /// * `reachability` - Checker for peer reachability
    /// * `filter` - Additional filter for candidates
    pub fn build_candidates_filtered<F>(
        &self,
        context: &RelayContext,
        reachability: &impl ReachabilityChecker,
        filter: F,
    ) -> Vec<RelayCandidate>
    where
        F: Fn(&RelayCandidate) -> bool,
    {
        self.build_candidates(context, reachability)
            .into_iter()
            .filter(filter)
            .collect()
    }

    /// Get only reachable candidates.
    pub fn build_reachable_candidates(
        &self,
        context: &RelayContext,
        reachability: &impl ReachabilityChecker,
    ) -> Vec<RelayCandidate> {
        self.build_candidates_filtered(context, reachability, |c| c.reachable)
    }

    /// Get the underlying topology.
    pub fn topology(&self) -> &SocialTopology {
        &self.social
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::{effects::relay::RelayRelationship, identifiers::ContextId};
    use crate::facts::HomeId;

    fn test_authority(seed: u8) -> AuthorityId {
        AuthorityId::new_from_entropy([seed; 32])
    }

    fn test_context(source: AuthorityId, destination: AuthorityId) -> RelayContext {
        RelayContext::new(
            ContextId::new_from_entropy([1u8; 32]),
            source,
            destination,
            1,
            [0u8; 32],
        )
    }

    struct AlwaysReachable;
    impl ReachabilityChecker for AlwaysReachable {
        fn is_reachable(&self, _peer: &AuthorityId) -> bool {
            true
        }
    }

    struct NeverReachable;
    impl ReachabilityChecker for NeverReachable {
        fn is_reachable(&self, _peer: &AuthorityId) -> bool {
            false
        }
    }

    #[test]
    fn test_build_candidates_empty_topology() {
        let local = test_authority(1);
        let target = test_authority(99);

        let topology = SocialTopology::empty(local);
        let builder = RelayCandidateBuilder::from_topology(topology);
        let context = test_context(local, target);

        let candidates = builder.build_candidates(&context, &AlwaysReachable);
        assert!(candidates.is_empty());
    }

    #[test]
    fn test_build_candidates_with_peers() {
        let local = test_authority(1);
        let peer1 = test_authority(2);
        let peer2 = test_authority(3);
        let target = test_authority(99);

        let home_id = HomeId::from_bytes([1u8; 32]);

        let mut topology = SocialTopology::empty(local);
        topology.add_peer(
            peer1,
            RelayRelationship::HomePeer {
                home_id: *home_id.as_bytes(),
            },
        );
        topology.add_peer(
            peer2,
            RelayRelationship::HomePeer {
                home_id: *home_id.as_bytes(),
            },
        );

        let builder = RelayCandidateBuilder::from_topology(topology);
        let context = test_context(local, target);

        let candidates = builder.build_candidates(&context, &AlwaysReachable);
        assert_eq!(candidates.len(), 2);
        assert!(candidates.iter().all(|c| c.reachable));
    }

    #[test]
    fn test_build_reachable_candidates_filters() {
        let local = test_authority(1);
        let peer1 = test_authority(2);
        let target = test_authority(99);

        let home_id = HomeId::from_bytes([1u8; 32]);

        let mut topology = SocialTopology::empty(local);
        topology.add_peer(
            peer1,
            RelayRelationship::HomePeer {
                home_id: *home_id.as_bytes(),
            },
        );

        let builder = RelayCandidateBuilder::from_topology(topology);
        let context = test_context(local, target);

        // With AlwaysReachable, we get the candidate
        let candidates = builder.build_reachable_candidates(&context, &AlwaysReachable);
        assert_eq!(candidates.len(), 1);

        // With NeverReachable, we get no candidates
        let candidates = builder.build_reachable_candidates(&context, &NeverReachable);
        assert!(candidates.is_empty());
    }

    #[test]
    fn test_closure_reachability() {
        let local = test_authority(1);
        let peer1 = test_authority(2);
        let peer2 = test_authority(3);
        let target = test_authority(99);

        let home_id = HomeId::from_bytes([1u8; 32]);

        let mut topology = SocialTopology::empty(local);
        topology.add_peer(
            peer1,
            RelayRelationship::HomePeer {
                home_id: *home_id.as_bytes(),
            },
        );
        topology.add_peer(
            peer2,
            RelayRelationship::HomePeer {
                home_id: *home_id.as_bytes(),
            },
        );

        let builder = RelayCandidateBuilder::from_topology(topology);
        let context = test_context(local, target);

        // Only peer1 is reachable
        let reachability = |peer: &AuthorityId| *peer == peer1;
        let candidates = builder.build_reachable_candidates(&context, &reachability);
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].authority_id, peer1);
    }
}
