//! Acknowledgment Types for Explicit Delivery Confirmation
//!
//! This module provides types for tracking explicit per-peer delivery
//! acknowledgments. Unlike `Propagation` which tracks gossip sync status,
//! `Acknowledgment` tracks explicit confirmation from specific peers.
//!
//! # Propagation vs Acknowledgment
//!
//! | Aspect | Propagation | Acknowledgment |
//! |--------|-------------|----------------|
//! | What it tracks | Gossip sync reached peers | Peer explicitly confirmed |
//! | How it's known | Transport layer observes | Requires ack protocol |
//! | Granularity | Aggregate (count) | Per-peer with timestamp |
//! | Opt-in | Always available | Fact must request acks |
//! | Use case | "Is sync complete?" | "Did Alice receive this?" |
//!
//! A fact can be:
//! - `Propagation::Complete` but `Acknowledgment` empty (sync'd but no ack protocol)
//! - `Propagation::Local` but `Acknowledgment` has entries (ack before full sync)

use crate::time::PhysicalTime;
use crate::types::AuthorityId;
use serde::{Deserialize, Serialize};

// ─────────────────────────────────────────────────────────────────────────────
// Acknowledgment Record
// ─────────────────────────────────────────────────────────────────────────────

/// A single acknowledgment record from a peer.
///
/// Records when a specific peer acknowledged receipt of a fact.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AckRecord {
    /// The peer who acknowledged
    pub peer: AuthorityId,

    /// When they acknowledged (their reported time)
    pub acked_at: PhysicalTime,
}

impl AckRecord {
    /// Create a new ack record
    pub fn new(peer: AuthorityId, acked_at: PhysicalTime) -> Self {
        Self { peer, acked_at }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Acknowledgment Collection
// ─────────────────────────────────────────────────────────────────────────────

/// Explicit acknowledgment from peers.
///
/// Only present for facts that opt into ack tracking. Contains a list
/// of all peers who have explicitly acknowledged receipt of the fact.
///
/// # Usage
///
/// ```ignore
/// let ack = Acknowledgment::new();
/// let ack = ack.add_ack(peer_id, now);
///
/// if ack.contains(&target_peer) {
///     println!("Target peer has acknowledged!");
/// }
///
/// if ack.count() >= required_threshold {
///     println!("Enough acknowledgments received");
/// }
/// ```
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Acknowledgment {
    /// Peers who explicitly acknowledged receipt
    pub acked_by: Vec<AckRecord>,
}

impl Acknowledgment {
    /// Create an empty acknowledgment tracker
    pub fn new() -> Self {
        Self::default()
    }

    /// Create from a list of ack records
    pub fn from_records(records: Vec<AckRecord>) -> Self {
        Self { acked_by: records }
    }

    /// Add an acknowledgment from a peer
    ///
    /// If the peer has already acknowledged, this updates their timestamp.
    #[must_use]
    pub fn add_ack(mut self, peer: AuthorityId, acked_at: PhysicalTime) -> Self {
        // Check if peer already acked
        if let Some(existing) = self.acked_by.iter_mut().find(|r| r.peer == peer) {
            // Update timestamp if newer
            if acked_at > existing.acked_at {
                existing.acked_at = acked_at;
            }
        } else {
            self.acked_by.push(AckRecord::new(peer, acked_at));
        }
        self
    }

    /// Add an acknowledgment in place
    pub fn record_ack(&mut self, peer: AuthorityId, acked_at: PhysicalTime) {
        if let Some(existing) = self.acked_by.iter_mut().find(|r| r.peer == peer) {
            if acked_at > existing.acked_at {
                existing.acked_at = acked_at;
            }
        } else {
            self.acked_by.push(AckRecord::new(peer, acked_at));
        }
    }

    /// Iterate over peer IDs that have acknowledged
    pub fn peers(&self) -> impl Iterator<Item = &AuthorityId> {
        self.acked_by.iter().map(|r| &r.peer)
    }

    /// Iterate over all ack records
    pub fn records(&self) -> impl Iterator<Item = &AckRecord> {
        self.acked_by.iter()
    }

    /// Check if a specific peer has acknowledged
    pub fn contains(&self, peer: &AuthorityId) -> bool {
        self.acked_by.iter().any(|r| &r.peer == peer)
    }

    /// Get the ack record for a specific peer
    pub fn get(&self, peer: &AuthorityId) -> Option<&AckRecord> {
        self.acked_by.iter().find(|r| &r.peer == peer)
    }

    /// Get the number of acknowledgments
    pub fn count(&self) -> usize {
        self.acked_by.len()
    }

    /// Check if no acknowledgments have been received
    pub fn is_empty(&self) -> bool {
        self.acked_by.is_empty()
    }

    /// Check if all expected peers have acknowledged
    pub fn all_acked(&self, expected: &[AuthorityId]) -> bool {
        expected.iter().all(|p| self.contains(p))
    }

    /// Get the earliest acknowledgment time
    pub fn earliest_ack(&self) -> Option<PhysicalTime> {
        self.acked_by.iter().map(|r| r.acked_at.clone()).min()
    }

    /// Get the latest acknowledgment time
    pub fn latest_ack(&self) -> Option<PhysicalTime> {
        self.acked_by.iter().map(|r| r.acked_at.clone()).max()
    }

    /// Get peers who have NOT acknowledged from an expected set
    pub fn missing_acks<'a>(&'a self, expected: &'a [AuthorityId]) -> Vec<&'a AuthorityId> {
        expected
            .iter()
            .filter(|p| !self.contains(p))
            .collect()
    }

    /// Merge another acknowledgment set into this one
    ///
    /// Takes the latest timestamp for each peer.
    #[must_use]
    pub fn merge(mut self, other: &Acknowledgment) -> Self {
        for record in &other.acked_by {
            self.record_ack(record.peer, record.acked_at.clone());
        }
        self
    }
}

impl FromIterator<AckRecord> for Acknowledgment {
    fn from_iter<T: IntoIterator<Item = AckRecord>>(iter: T) -> Self {
        Self {
            acked_by: iter.into_iter().collect(),
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn test_authority(n: u8) -> AuthorityId {
        AuthorityId::from_uuid(Uuid::from_bytes([n; 16]))
    }

    fn test_time(millis: u64) -> PhysicalTime {
        PhysicalTime {
            ts_ms: millis,
            uncertainty: None,
        }
    }

    #[test]
    fn test_ack_record() {
        let peer = test_authority(1);
        let time = test_time(1000);
        let record = AckRecord::new(peer, time.clone());

        assert_eq!(record.peer, peer);
        assert_eq!(record.acked_at, time);
    }

    #[test]
    fn test_acknowledgment_empty() {
        let ack = Acknowledgment::new();
        assert!(ack.is_empty());
        assert_eq!(ack.count(), 0);
        assert!(ack.earliest_ack().is_none());
        assert!(ack.latest_ack().is_none());
    }

    #[test]
    fn test_acknowledgment_add() {
        let peer1 = test_authority(1);
        let peer2 = test_authority(2);

        let ack = Acknowledgment::new()
            .add_ack(peer1, test_time(1000))
            .add_ack(peer2, test_time(2000));

        assert_eq!(ack.count(), 2);
        assert!(ack.contains(&peer1));
        assert!(ack.contains(&peer2));
        assert!(!ack.contains(&test_authority(3)));
    }

    #[test]
    fn test_acknowledgment_duplicate_updates_time() {
        let peer = test_authority(1);

        let ack = Acknowledgment::new()
            .add_ack(peer, test_time(1000))
            .add_ack(peer, test_time(2000));

        // Should only have one record with updated time
        assert_eq!(ack.count(), 1);
        assert_eq!(ack.get(&peer).unwrap().acked_at, test_time(2000));
    }

    #[test]
    fn test_acknowledgment_peers_iterator() {
        let peer1 = test_authority(1);
        let peer2 = test_authority(2);

        let ack = Acknowledgment::new()
            .add_ack(peer1, test_time(1000))
            .add_ack(peer2, test_time(2000));

        let peers: Vec<_> = ack.peers().collect();
        assert_eq!(peers.len(), 2);
        assert!(peers.contains(&&peer1));
        assert!(peers.contains(&&peer2));
    }

    #[test]
    fn test_acknowledgment_all_acked() {
        let peer1 = test_authority(1);
        let peer2 = test_authority(2);
        let peer3 = test_authority(3);

        let ack = Acknowledgment::new()
            .add_ack(peer1, test_time(1000))
            .add_ack(peer2, test_time(2000));

        let expected12 = vec![peer1, peer2];
        let expected123 = vec![peer1, peer2, peer3];

        assert!(ack.all_acked(&expected12));
        assert!(!ack.all_acked(&expected123));
    }

    #[test]
    fn test_acknowledgment_missing_acks() {
        let peer1 = test_authority(1);
        let peer2 = test_authority(2);
        let peer3 = test_authority(3);

        let ack = Acknowledgment::new().add_ack(peer1, test_time(1000));

        let expected = vec![peer1, peer2, peer3];
        let missing = ack.missing_acks(&expected);

        assert_eq!(missing.len(), 2);
        assert!(missing.contains(&&peer2));
        assert!(missing.contains(&&peer3));
    }

    #[test]
    fn test_acknowledgment_earliest_latest() {
        let ack = Acknowledgment::new()
            .add_ack(test_authority(1), test_time(1000))
            .add_ack(test_authority(2), test_time(3000))
            .add_ack(test_authority(3), test_time(2000));

        assert_eq!(ack.earliest_ack(), Some(test_time(1000)));
        assert_eq!(ack.latest_ack(), Some(test_time(3000)));
    }

    #[test]
    fn test_acknowledgment_merge() {
        let peer1 = test_authority(1);
        let peer2 = test_authority(2);

        let ack1 = Acknowledgment::new().add_ack(peer1, test_time(1000));

        let ack2 = Acknowledgment::new()
            .add_ack(peer1, test_time(2000)) // Later time for same peer
            .add_ack(peer2, test_time(1500));

        let merged = ack1.merge(&ack2);

        assert_eq!(merged.count(), 2);
        // peer1 should have the later time
        assert_eq!(merged.get(&peer1).unwrap().acked_at, test_time(2000));
        assert_eq!(merged.get(&peer2).unwrap().acked_at, test_time(1500));
    }

    #[test]
    fn test_acknowledgment_from_iterator() {
        let records = vec![
            AckRecord::new(test_authority(1), test_time(1000)),
            AckRecord::new(test_authority(2), test_time(2000)),
        ];

        let ack: Acknowledgment = records.into_iter().collect();
        assert_eq!(ack.count(), 2);
    }
}
