//! Layer 1: Relay Selection Effect Trait Definitions
//!
//! This module defines the pure trait interface for relay selection in Aura's
//! social infrastructure. Relay is **neighborhood-scoped**: both home peers
//! and neighborhood peers can relay for anyone in the neighborhood.
//!
//! **Effect Classification**: Application Effect
//! - Implemented by transport crates (aura-transport provides deterministic selector)
//! - Used by protocol layer (aura-protocol) for relay orchestration
//! - Core trait definition belongs in Layer 1 (foundation)
//!
//! # Design Principles
//!
//! **Tiered selection**: Home peers are preferred (closest trust), then
//! neighborhood peers, then guardians as fallback.
//!
//! **Deterministic**: Selection uses `hash(context_id, epoch, nonce)` for
//! reproducible results in testing and simulation.
//!
//! **Neighborhood-scoped**: All relay relationships operate at neighborhood
//! scope - anyone in the neighborhood can relay for anyone else.

use crate::types::identifiers::{AuthorityId, ContextId};
use serde::{Deserialize, Serialize};

/// Context for relay selection decisions.
///
/// Contains all information needed to select appropriate relay nodes
/// for a message. The context is used to compute deterministic selection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RelayContext {
    /// The relational context scoping this relay operation.
    pub context_id: ContextId,
    /// The authority sending the message.
    pub source: AuthorityId,
    /// The authority receiving the message.
    pub destination: AuthorityId,
    /// Current epoch (for deterministic selection).
    pub epoch: u64,
    /// Message nonce (for deterministic randomness).
    ///
    /// Combined with context_id and epoch, this ensures different messages
    /// select different relays while remaining reproducible.
    pub nonce: [u8; 32],
}

impl RelayContext {
    /// Create a new relay context.
    pub fn new(
        context_id: ContextId,
        source: AuthorityId,
        destination: AuthorityId,
        epoch: u64,
        nonce: [u8; 32],
    ) -> Self {
        Self {
            context_id,
            source,
            destination,
            epoch,
            nonce,
        }
    }
}

/// How we know a potential relay peer.
///
/// Relay capability derives from social relationships. The relationship
/// type affects selection priority: home peers are preferred over
/// neighborhood peers, which are preferred over guardians.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RelayRelationship {
    /// Co-resident in the same home.
    ///
    /// Home peers share home context and have high mutual trust.
    /// They can relay for any member of shared neighborhoods.
    HomePeer {
        /// The home ID (opaque 32-byte identifier)
        home_id: [u8; 32],
    },

    /// Member of an adjacent home in a shared neighborhood.
    ///
    /// Neighborhood peers share neighborhood context and have
    /// established traversal rights. They can relay for any
    /// member of the neighborhood.
    NeighborhoodPeer {
        /// The neighborhood ID (opaque 32-byte identifier)
        neighborhood_id: [u8; 32],
    },

    /// Designated guardian with explicit relay capability.
    ///
    /// Guardians are the fallback when social topology doesn't
    /// provide a relay path. They have explicit capability grants
    /// for relay operations.
    Guardian,
}

impl RelayRelationship {
    /// Get the priority of this relationship type.
    ///
    /// Lower values are higher priority (selected first).
    pub fn priority(&self) -> u8 {
        match self {
            Self::HomePeer { .. } => 0,        // Highest priority
            Self::NeighborhoodPeer { .. } => 1, // Medium priority
            Self::Guardian => 2,                // Fallback
        }
    }

    /// Check if this is a home peer relationship.
    pub fn is_home_peer(&self) -> bool {
        matches!(self, Self::HomePeer { .. })
    }

    /// Check if this is a neighborhood peer relationship.
    pub fn is_neighborhood_peer(&self) -> bool {
        matches!(self, Self::NeighborhoodPeer { .. })
    }

    /// Check if this is a guardian relationship.
    pub fn is_guardian(&self) -> bool {
        matches!(self, Self::Guardian)
    }
}

/// A candidate relay peer.
///
/// Contains information about a potential relay, including the
/// relationship type and current reachability status.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RelayCandidate {
    /// The potential relay authority.
    pub authority_id: AuthorityId,
    /// How we know this relay (determines selection priority).
    pub relationship: RelayRelationship,
    /// Is this peer currently reachable?
    ///
    /// Unreachable peers are excluded from selection but may
    /// be included in fallback lists.
    pub reachable: bool,
}

impl RelayCandidate {
    /// Create a new relay candidate.
    pub fn new(
        authority_id: AuthorityId,
        relationship: RelayRelationship,
        reachable: bool,
    ) -> Self {
        Self {
            authority_id,
            relationship,
            reachable,
        }
    }

    /// Create a reachable home peer candidate.
    pub fn block_peer(authority_id: AuthorityId, home_id: [u8; 32]) -> Self {
        Self::new(
            authority_id,
            RelayRelationship::HomePeer { home_id },
            true,
        )
    }

    /// Create a reachable neighborhood peer candidate.
    pub fn neighborhood_peer(authority_id: AuthorityId, neighborhood_id: [u8; 32]) -> Self {
        Self::new(
            authority_id,
            RelayRelationship::NeighborhoodPeer { neighborhood_id },
            true,
        )
    }

    /// Create a reachable guardian candidate.
    pub fn guardian(authority_id: AuthorityId) -> Self {
        Self::new(authority_id, RelayRelationship::Guardian, true)
    }
}

/// Error type for relay operations.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RelayError {
    /// No relay candidates available.
    NoCandidates,

    /// All relays failed.
    AllRelaysFailed {
        /// Number of relays that were tried
        relays_tried: u32,
    },

    /// Relay rejected the request.
    RelayRejected {
        /// The relay that rejected
        relay: AuthorityId,
        /// Reason for rejection
        reason: String,
    },

    /// Budget exhausted for relay operations.
    BudgetExhausted,

    /// Network error during relay.
    NetworkError(String),
}

impl std::fmt::Display for RelayError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoCandidates => write!(f, "no relay candidates available"),
            Self::AllRelaysFailed { relays_tried } => {
                write!(f, "all {relays_tried} relays failed")
            }
            Self::RelayRejected { relay, reason } => {
                write!(f, "relay {relay} rejected: {reason}")
            }
            Self::BudgetExhausted => write!(f, "relay budget exhausted"),
            Self::NetworkError(msg) => write!(f, "network error: {msg}"),
        }
    }
}

impl std::error::Error for RelayError {}

/// Strategy for selecting relay nodes.
///
/// Implementations determine how relays are selected from candidates.
/// The default implementation uses deterministic random selection
/// with tier-based priority.
///
/// # Implementation Notes
///
/// Implementations should:
/// - Filter out unreachable candidates (unless building fallback lists)
/// - Prefer candidates by relationship priority (home > neighborhood > guardian)
/// - Use deterministic selection for reproducibility in tests
/// - Return an ordered list: first choice, then fallbacks
///
/// # Example
///
/// ```ignore
/// // Deterministic random selection
/// impl RelaySelector for DeterministicRandomSelector {
///     fn select(&self, context: &RelayContext, candidates: &[RelayCandidate]) -> Vec<AuthorityId> {
///         let reachable: Vec<_> = candidates.iter().filter(|c| c.reachable).collect();
///         let seed = hash(&[context.context_id, context.epoch, context.nonce]);
///         select_by_tiers(&reachable, &seed)
///     }
/// }
/// ```
pub trait RelaySelector: Send + Sync {
    /// Select relay(s) from candidates.
    ///
    /// Returns an ordered list of relay authorities to try:
    /// - First element is the primary relay
    /// - Subsequent elements are fallbacks in order of preference
    ///
    /// # Arguments
    /// * `context` - The relay context (used for deterministic selection)
    /// * `candidates` - Available relay candidates
    ///
    /// # Returns
    /// An ordered list of authority IDs to use as relays.
    /// Empty if no suitable candidates are available.
    fn select(&self, context: &RelayContext, candidates: &[RelayCandidate]) -> Vec<AuthorityId>;
}

/// Blanket implementation for Arc<T> where T: RelaySelector
impl<T: RelaySelector + ?Sized> RelaySelector for std::sync::Arc<T> {
    fn select(&self, context: &RelayContext, candidates: &[RelayCandidate]) -> Vec<AuthorityId> {
        (**self).select(context, candidates)
    }
}

/// Blanket implementation for Box<T> where T: RelaySelector
impl<T: RelaySelector + ?Sized> RelaySelector for Box<T> {
    fn select(&self, context: &RelayContext, candidates: &[RelayCandidate]) -> Vec<AuthorityId> {
        (**self).select(context, candidates)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn test_authority() -> AuthorityId {
        AuthorityId::from_uuid(Uuid::from_bytes([1u8; 16]))
    }

    fn test_context() -> RelayContext {
        RelayContext::new(
            ContextId::from_uuid(Uuid::from_bytes([2u8; 16])),
            AuthorityId::from_uuid(Uuid::from_bytes([3u8; 16])),
            AuthorityId::from_uuid(Uuid::from_bytes([4u8; 16])),
            1,
            [0u8; 32],
        )
    }

    #[test]
    fn test_relay_relationship_priority() {
        let home_rel = RelayRelationship::HomePeer {
            home_id: [1u8; 32],
        };
        let neighborhood = RelayRelationship::NeighborhoodPeer {
            neighborhood_id: [2u8; 32],
        };
        let guardian = RelayRelationship::Guardian;

        assert!(home_rel.priority() < neighborhood.priority());
        assert!(neighborhood.priority() < guardian.priority());
    }

    #[test]
    fn test_relay_relationship_checks() {
        let home_rel = RelayRelationship::HomePeer {
            home_id: [1u8; 32],
        };
        assert!(home_rel.is_home_peer());
        assert!(!home_rel.is_neighborhood_peer());
        assert!(!home_rel.is_guardian());

        let neighborhood = RelayRelationship::NeighborhoodPeer {
            neighborhood_id: [2u8; 32],
        };
        assert!(!neighborhood.is_home_peer());
        assert!(neighborhood.is_neighborhood_peer());
        assert!(!neighborhood.is_guardian());

        let guardian = RelayRelationship::Guardian;
        assert!(!guardian.is_home_peer());
        assert!(!guardian.is_neighborhood_peer());
        assert!(guardian.is_guardian());
    }

    #[test]
    fn test_relay_candidate_constructors() {
        let auth = test_authority();

        let block_peer = RelayCandidate::block_peer(auth, [1u8; 32]);
        assert!(block_peer.reachable);
        assert!(block_peer.relationship.is_home_peer());

        let neighborhood_peer = RelayCandidate::neighborhood_peer(auth, [2u8; 32]);
        assert!(neighborhood_peer.reachable);
        assert!(neighborhood_peer.relationship.is_neighborhood_peer());

        let guardian = RelayCandidate::guardian(auth);
        assert!(guardian.reachable);
        assert!(guardian.relationship.is_guardian());
    }

    #[test]
    fn test_relay_context_creation() {
        let ctx = test_context();
        assert_eq!(ctx.epoch, 1);
        assert_eq!(ctx.nonce, [0u8; 32]);
    }

    #[test]
    fn test_relay_error_display() {
        let no_candidates = RelayError::NoCandidates;
        assert!(no_candidates.to_string().contains("no relay candidates"));

        let all_failed = RelayError::AllRelaysFailed { relays_tried: 3 };
        assert!(all_failed.to_string().contains("3 relays failed"));

        let rejected = RelayError::RelayRejected {
            relay: test_authority(),
            reason: "busy".to_string(),
        };
        assert!(rejected.to_string().contains("rejected"));

        let exhausted = RelayError::BudgetExhausted;
        assert!(exhausted.to_string().contains("budget exhausted"));
    }
}
