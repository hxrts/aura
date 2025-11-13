//! PeerView - Grow-Only Set of Known Peers
//!
//! Implements a GSet (Grow-Only Set) CRDT for tracking discovered peers
//! in the tree synchronization network. PeerView monotonically grows as
//! new peers are discovered through anti-entropy or gossip.

#![allow(clippy::disallowed_methods)] // TODO: Replace direct UUID calls with effect system
//!
//! ## Design Principles
//!
//! - **Grow-Only**: Peers can only be added, never removed
//! - **Join-Semilattice**: Union operation for merging peer sets
//! - **Idempotent**: Multiple additions of same peer are idempotent
//! - **No Tombstones**: No removal tracking needed (pure growth)
//!
//! ## Usage
//!
//! ```rust,ignore
//! use crate::sync::PeerView;
//! use uuid::Uuid;
//!
//! let mut view1 = PeerView::new();
//! let mut view2 = PeerView::new();
//!
//! view1.add_peer(peer_a);
//! view2.add_peer(peer_b);
//!
//! // Merge views via join
//! let merged = view1.join(&view2);
//! assert!(merged.contains(&peer_a));
//! assert!(merged.contains(&peer_b));
//! ```

use aura_core::semilattice::{Bottom, JoinSemilattice};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use uuid::Uuid;

/// Grow-only set of discovered peers
///
/// PeerView implements a GSet CRDT where peers can only be added,
/// never removed. This provides monotonic growth of the peer set
/// with eventual consistency guarantees.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PeerView {
    /// Set of known peer IDs
    peers: BTreeSet<Uuid>,
}

impl PeerView {
    /// Create a new empty peer view
    pub fn new() -> Self {
        Self {
            peers: BTreeSet::new(),
        }
    }

    /// Add a peer to the view
    ///
    /// This operation is idempotent - adding the same peer multiple times
    /// has no additional effect.
    pub fn add_peer(&mut self, peer_id: Uuid) {
        self.peers.insert(peer_id);
    }

    /// Check if a peer is in the view
    pub fn contains(&self, peer_id: &Uuid) -> bool {
        self.peers.contains(peer_id)
    }

    /// Get the number of known peers
    pub fn len(&self) -> usize {
        self.peers.len()
    }

    /// Check if the view is empty
    pub fn is_empty(&self) -> bool {
        self.peers.is_empty()
    }

    /// Iterate over all peer IDs
    pub fn iter(&self) -> impl Iterator<Item = &Uuid> {
        self.peers.iter()
    }

    /// Get all peer IDs as a vector
    pub fn to_vec(&self) -> Vec<Uuid> {
        self.peers.iter().copied().collect()
    }

    /// Create a PeerView from a collection of peer IDs
    pub fn from_peers<I>(peers: I) -> Self
    where
        I: IntoIterator<Item = Uuid>,
    {
        Self {
            peers: peers.into_iter().collect(),
        }
    }
}

impl Default for PeerView {
    fn default() -> Self {
        Self::new()
    }
}

impl JoinSemilattice for PeerView {
    /// Join two peer views via set union
    ///
    /// The result contains all peers from both views.
    /// This operation is:
    /// - Associative: (a ⊔ b) ⊔ c = a ⊔ (b ⊔ c)
    /// - Commutative: a ⊔ b = b ⊔ a
    /// - Idempotent: a ⊔ a = a
    fn join(&self, other: &Self) -> Self {
        let mut result = self.clone();
        for peer in &other.peers {
            result.peers.insert(*peer);
        }
        result
    }
}

impl Bottom for PeerView {
    /// Bottom element is the empty peer set
    fn bottom() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_add_peer() {
        let mut view = PeerView::new();
        let peer = Uuid::new_v4();

        view.add_peer(peer);
        assert!(view.contains(&peer));
        assert_eq!(view.len(), 1);
    }

    #[test]
    fn test_add_peer_idempotent() {
        let mut view = PeerView::new();
        let peer = Uuid::new_v4();

        view.add_peer(peer);
        view.add_peer(peer); // Add twice
        assert_eq!(view.len(), 1); // Still only one
    }

    #[test]
    fn test_join_associative() {
        let peer_a = Uuid::new_v4();
        let peer_b = Uuid::new_v4();
        let peer_c = Uuid::new_v4();

        let view_a = PeerView::from_peers(vec![peer_a]);
        let view_b = PeerView::from_peers(vec![peer_b]);
        let view_c = PeerView::from_peers(vec![peer_c]);

        let left = view_a.join(&view_b).join(&view_c);
        let right = view_a.join(&view_b.join(&view_c));

        assert_eq!(left, right);
    }

    #[test]
    fn test_join_commutative() {
        let peer_a = Uuid::new_v4();
        let peer_b = Uuid::new_v4();

        let view_a = PeerView::from_peers(vec![peer_a]);
        let view_b = PeerView::from_peers(vec![peer_b]);

        assert_eq!(view_a.join(&view_b), view_b.join(&view_a));
    }

    #[test]
    fn test_join_idempotent() {
        let peer = Uuid::new_v4();
        let view = PeerView::from_peers(vec![peer]);

        assert_eq!(view.join(&view), view);
    }

    #[test]
    fn test_bottom() {
        let view = PeerView::bottom();
        assert!(view.is_empty());
        assert_eq!(view.len(), 0);
    }

    #[test]
    fn test_join_with_bottom() {
        let peer = Uuid::new_v4();
        let view = PeerView::from_peers(vec![peer]);
        let bottom = PeerView::bottom();

        // Join with bottom should be identity
        assert_eq!(view.join(&bottom), view);
        assert_eq!(bottom.join(&view), view);
    }
}
