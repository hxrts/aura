//! SSB Gossip Protocol Implementation
//!
//! Implements the gossip protocol for SSB envelope distribution with:
//! - Active neighbor management with trust-based selection
//! - Rate limiting and exponential backoff for failed merges
//! - Peer discovery through neighbor lists
//! - Envelope expiry and garbage collection

use std::collections::BTreeMap;

use crate::error::{TransportErrorBuilder, TransportResult};
use crate::infrastructure::envelope::{Cid, Envelope};

/// Peer identifier (device ID)
pub type PeerId = Vec<u8>;

/// Account identifier
pub type AccountId = Vec<u8>;

/// Trust level for peer relationships
///
/// Determines how much we trust a peer for gossip operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrustLevel {
    /// Direct relationship (highest trust)
    Direct,
    /// One degree of separation
    OneDegree,
    /// Two degrees of separation (lowest trust)
    TwoDegree,
}

/// Authentication information for a peer
///
/// Contains verified device and account identifiers with trust level.
#[derive(Debug, Clone)]
pub struct PeerAuthentication {
    /// Device identifier for this peer
    pub device_id: Vec<u8>,
    /// Account this device belongs to
    pub account_id: AccountId,
    /// Last successful authentication timestamp
    pub last_authenticated: u64,
    /// Trust level for this peer
    pub trust_level: TrustLevel,
}

/// Permissions granted to a peer
///
/// Controls what operations this peer can perform through our node.
#[derive(Debug, Clone)]
pub struct PeerPermissions {
    /// Relay operation permissions
    pub relay_permissions: Vec<String>,
    /// Communication operation permissions
    pub communication_permissions: Vec<String>,
    /// Storage operation permissions
    pub storage_permissions: Vec<String>,
    /// Granted capability tokens
    pub granted_capabilities: Vec<Vec<u8>>,
    /// Last permission update timestamp
    pub last_permission_update: u64,
}

/// Information about an active gossip neighbor
///
/// Active neighbors are peers we actively exchange envelopes with.
#[derive(Debug, Clone)]
pub struct NeighborInfo {
    /// Peer identifier
    pub peer_id: PeerId,
    /// Authentication information
    pub authentication: PeerAuthentication,
    /// Granted permissions
    pub permissions: PeerPermissions,
    /// When this neighbor was added
    pub added_at: u64,
}

/// History of merge operations with a peer
///
/// Tracks success/failure for rate limiting and backoff.
#[derive(Debug, Clone)]
pub struct MergeHistory {
    /// Last successful merge timestamp
    pub last_merge_at: u64,
    /// Total number of successful merges
    pub merge_count: u64,
    /// Number of consecutive failures
    pub consecutive_failures: u32,
    /// Timestamp until which peer is backed off
    pub backoff_until: u64,
}

impl MergeHistory {
    /// Create new merge history
    pub fn new(_now: u64) -> Self {
        Self {
            last_merge_at: 0,
            merge_count: 0,
            consecutive_failures: 0,
            backoff_until: 0,
        }
    }

    /// Record a successful merge
    pub fn record_success(&mut self, now: u64) {
        self.last_merge_at = now;
        self.merge_count += 1;
        self.consecutive_failures = 0;
        self.backoff_until = now;
    }

    /// Record a failed merge with exponential backoff
    pub fn record_failure(&mut self, now: u64) {
        self.consecutive_failures += 1;
        let backoff_ms = 1000u64 * 2u64.pow(self.consecutive_failures.min(10));
        self.backoff_until = now + backoff_ms;
    }

    /// Check if merge is allowed (not in backoff period)
    pub fn can_merge(&self, now: u64) -> bool {
        now >= self.backoff_until
    }
}

/// Rate limiter for merge operations
///
/// Prevents excessive merge attempts by enforcing minimum intervals.
pub struct RateLimiter {
    min_merge_interval_ms: u64,
}

impl RateLimiter {
    /// Create new rate limiter with minimum merge interval
    pub fn new(min_merge_interval_ms: u64) -> Self {
        Self {
            min_merge_interval_ms,
        }
    }

    /// Check if merge is allowed under rate limit
    pub fn check_rate_limit(&self, history: &MergeHistory, now: u64) -> bool {
        if !history.can_merge(now) {
            return false;
        }
        let elapsed = now.saturating_sub(history.last_merge_at);
        elapsed >= self.min_merge_interval_ms
    }
}

/// Metadata about a locally stored envelope
#[derive(Debug, Clone)]
pub struct EnvelopeMetadata {
    /// Content identifier for this envelope
    pub cid: Cid,
    /// When envelope was added to local store
    pub added_at: u64,
    /// Epoch at which this envelope expires
    pub expires_at_epoch: u64,
    /// Size in bytes
    pub size_bytes: usize,
}

/// SSB Gossip protocol implementation
///
/// Manages:
/// - Active neighbors for envelope exchange
/// - Known peers for potential promotion to neighbors
/// - Local envelope storage with expiry
/// - Rate limiting and backoff for merge operations
pub struct SbbGossip {
    active_neighbors: BTreeMap<PeerId, NeighborInfo>,
    known_peers: BTreeMap<PeerId, PeerAuthentication>,
    merge_history: BTreeMap<PeerId, MergeHistory>,
    local_envelopes: BTreeMap<Cid, EnvelopeMetadata>,
    rate_limiter: RateLimiter,
    max_active_neighbors: usize,
}

impl SbbGossip {
    /// Create new SSB gossip instance
    ///
    /// # Arguments
    /// * `max_active_neighbors` - Maximum number of active gossip neighbors
    /// * `min_merge_interval_ms` - Minimum milliseconds between merge attempts
    pub fn new(max_active_neighbors: usize, min_merge_interval_ms: u64) -> Self {
        Self {
            active_neighbors: BTreeMap::new(),
            known_peers: BTreeMap::new(),
            merge_history: BTreeMap::new(),
            local_envelopes: BTreeMap::new(),
            rate_limiter: RateLimiter::new(min_merge_interval_ms),
            max_active_neighbors,
        }
    }

    /// Add a peer as an active gossip neighbor
    ///
    /// Initializes merge history for this neighbor.
    pub fn add_active_neighbor(&mut self, neighbor: NeighborInfo, now: u64) {
        let peer_id = neighbor.peer_id.clone();
        self.active_neighbors.insert(peer_id.clone(), neighbor);
        self.merge_history
            .entry(peer_id)
            .or_insert_with(|| MergeHistory::new(now));
    }

    /// Remove a peer from active neighbors
    pub fn remove_active_neighbor(&mut self, peer_id: &PeerId) {
        self.active_neighbors.remove(peer_id);
    }

    /// Add a peer to known peers list
    ///
    /// Known peers can be promoted to active neighbors later.
    pub fn add_known_peer(&mut self, peer_id: PeerId, auth: PeerAuthentication) {
        self.known_peers.insert(peer_id, auth);
    }

    /// Publish an envelope to active neighbors
    ///
    /// Adds envelope to local storage and initiates eager push to neighbors
    /// that are within rate limits.
    ///
    /// Returns list of peers the envelope was pushed to.
    pub fn publish_envelope(
        &mut self,
        cid: Cid,
        _envelope: &Envelope,
        expires_at_epoch: u64,
        now: u64,
    ) -> TransportResult<Vec<PeerId>> {
        let metadata = EnvelopeMetadata {
            cid: cid.clone(),
            added_at: now,
            expires_at_epoch,
            size_bytes: 2048,
        };
        self.local_envelopes.insert(cid.clone(), metadata);

        self.eager_push_to_neighbors(now)
    }

    fn eager_push_to_neighbors(&mut self, now: u64) -> TransportResult<Vec<PeerId>> {
        if self.active_neighbors.is_empty() {
            return Err(TransportErrorBuilder::transport("No active neighbors"));
        }

        let mut pushed_to = Vec::new();

        for (peer_id, _neighbor) in self.active_neighbors.iter() {
            let history = self.merge_history.get(peer_id).unwrap();

            if self.rate_limiter.check_rate_limit(history, now) {
                pushed_to.push(peer_id.clone());
            }
        }

        Ok(pushed_to)
    }

    /// Initiate a merge operation with a neighbor
    ///
    /// Checks rate limits and backoff before allowing merge.
    pub fn initiate_merge_with_neighbor(
        &mut self,
        peer_id: &PeerId,
        now: u64,
    ) -> TransportResult<()> {
        if !self.active_neighbors.contains_key(peer_id) {
            return Err(TransportErrorBuilder::transport("Unknown peer"));
        }

        let history = self
            .merge_history
            .get(peer_id)
            .ok_or(TransportErrorBuilder::transport("Unknown peer"))?;

        if !self.rate_limiter.check_rate_limit(history, now) {
            return Err(TransportErrorBuilder::transport("Rate limit exceeded"));
        }

        if !history.can_merge(now) {
            return Err(TransportErrorBuilder::transport("Backoff active"));
        }

        Ok(())
    }

    /// Record successful merge with a neighbor
    ///
    /// Resets consecutive failure count and updates history.
    pub fn record_merge_success(&mut self, peer_id: &PeerId, now: u64) {
        self.merge_history
            .entry(peer_id.clone())
            .or_insert_with(|| MergeHistory::new(now))
            .record_success(now);
    }

    /// Record failed merge with a neighbor
    ///
    /// Applies exponential backoff and demotes neighbor after 3+ failures.
    pub fn record_merge_failure(&mut self, peer_id: &PeerId, now: u64) {
        self.merge_history
            .entry(peer_id.clone())
            .or_insert_with(|| MergeHistory::new(now))
            .record_failure(now);

        let failures = self
            .merge_history
            .get(peer_id)
            .map(|h| h.consecutive_failures)
            .unwrap_or(0);

        if failures >= 3 {
            self.demote_neighbor_to_known_peer(peer_id);
        }
    }

    fn demote_neighbor_to_known_peer(&mut self, peer_id: &PeerId) {
        if let Some(neighbor) = self.active_neighbors.remove(peer_id) {
            self.known_peers
                .insert(peer_id.clone(), neighbor.authentication);
        }
    }

    /// Promote a known peer to active neighbor
    ///
    /// Fails if maximum active neighbors limit is reached.
    pub fn promote_known_peer_to_neighbor(
        &mut self,
        peer_id: &PeerId,
        permissions: PeerPermissions,
        now: u64,
    ) -> TransportResult<()> {
        if self.active_neighbors.len() >= self.max_active_neighbors {
            return Err(TransportErrorBuilder::transport("No active neighbors"));
        }

        let auth = self
            .known_peers
            .get(peer_id)
            .ok_or(TransportErrorBuilder::transport("Unknown peer"))?
            .clone();

        let neighbor = NeighborInfo {
            peer_id: peer_id.clone(),
            authentication: auth,
            permissions,
            added_at: now,
        };

        self.add_active_neighbor(neighbor, now);
        self.known_peers.remove(peer_id);

        Ok(())
    }

    /// Discover new peers from a neighbor's peer list
    ///
    /// Adds unknown peers to known peers list.
    pub fn discover_peers_from_neighbor(
        &mut self,
        neighbor_peers: Vec<(PeerId, PeerAuthentication)>,
    ) {
        for (peer_id, auth) in neighbor_peers {
            if !self.active_neighbors.contains_key(&peer_id)
                && !self.known_peers.contains_key(&peer_id)
            {
                self.known_peers.insert(peer_id, auth);
            }
        }
    }

    /// Garbage collect expired envelopes
    ///
    /// Removes envelopes past their expiry epoch.
    /// Returns list of removed envelope CIDs.
    pub fn gc_expired_envelopes(&mut self, current_epoch: u64) -> Vec<Cid> {
        let mut expired = Vec::new();

        self.local_envelopes.retain(|cid, metadata| {
            if metadata.expires_at_epoch <= current_epoch {
                expired.push(cid.clone());
                false
            } else {
                true
            }
        });

        expired
    }

    /// Get number of active neighbors
    pub fn get_active_neighbor_count(&self) -> usize {
        self.active_neighbors.len()
    }

    /// Get number of known peers
    pub fn get_known_peer_count(&self) -> usize {
        self.known_peers.len()
    }

    /// Get number of locally stored envelopes
    pub fn get_envelope_count(&self) -> usize {
        self.local_envelopes.len()
    }

    /// Get merge history for a peer
    pub fn get_merge_history(&self, peer_id: &PeerId) -> Option<&MergeHistory> {
        self.merge_history.get(peer_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_peer_id(id: u8) -> PeerId {
        vec![id]
    }

    fn create_test_auth(id: u8, trust_level: TrustLevel) -> PeerAuthentication {
        PeerAuthentication {
            device_id: vec![id],
            account_id: vec![id],
            last_authenticated: 1000,
            trust_level,
        }
    }

    fn create_test_permissions() -> PeerPermissions {
        PeerPermissions {
            relay_permissions: vec![],
            communication_permissions: vec![],
            storage_permissions: vec![],
            granted_capabilities: vec![],
            last_permission_update: 1000,
        }
    }

    fn create_test_neighbor(id: u8) -> NeighborInfo {
        NeighborInfo {
            peer_id: create_test_peer_id(id),
            authentication: create_test_auth(id, TrustLevel::Direct),
            permissions: create_test_permissions(),
            added_at: 1000,
        }
    }

    fn create_test_envelope() -> Envelope {
        use crate::infrastructure::envelope::{Header, HeaderBare, RoutingTag};
        let header_bare = HeaderBare {
            version: 1,
            epoch: 100,
            counter: 1,
            rtag: RoutingTag([0u8; 16]),
            ttl_epochs: 10,
        };
        let cid = Cid([0u8; 32]);
        let header = Header {
            bare: header_bare,
            cid: cid.clone(),
        };
        Envelope {
            header,
            ciphertext: vec![0u8; 1920],
        }
    }

    #[test]
    fn test_add_active_neighbor() {
        let mut gossip = SbbGossip::new(8, 1000);
        let neighbor = create_test_neighbor(1);
        let peer_id = neighbor.peer_id.clone();

        gossip.add_active_neighbor(neighbor, 1000);

        assert_eq!(gossip.get_active_neighbor_count(), 1);
        assert!(gossip.active_neighbors.contains_key(&peer_id));
        assert!(gossip.merge_history.contains_key(&peer_id));
    }

    #[test]
    fn test_rate_limiting() {
        let mut gossip = SbbGossip::new(8, 1000);
        let neighbor = create_test_neighbor(1);
        let peer_id = neighbor.peer_id.clone();

        gossip.add_active_neighbor(neighbor, 1000);

        assert!(gossip.initiate_merge_with_neighbor(&peer_id, 1000).is_ok());

        gossip.record_merge_success(&peer_id, 1000);

        let result = gossip.initiate_merge_with_neighbor(&peer_id, 1500);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Rate limit exceeded"));

        assert!(gossip.initiate_merge_with_neighbor(&peer_id, 2000).is_ok());
    }

    #[test]
    fn test_exponential_backoff() {
        let mut gossip = SbbGossip::new(8, 1000);
        let neighbor = create_test_neighbor(1);
        let peer_id = neighbor.peer_id.clone();

        gossip.add_active_neighbor(neighbor, 1000);

        gossip.record_merge_failure(&peer_id, 1000);
        let history = gossip.get_merge_history(&peer_id).unwrap();
        assert_eq!(history.consecutive_failures, 1);
        assert_eq!(history.backoff_until, 3000);

        gossip.record_merge_failure(&peer_id, 3000);
        let history = gossip.get_merge_history(&peer_id).unwrap();
        assert_eq!(history.consecutive_failures, 2);
        assert_eq!(history.backoff_until, 7000);

        gossip.record_merge_success(&peer_id, 7000);
        let history = gossip.get_merge_history(&peer_id).unwrap();
        assert_eq!(history.consecutive_failures, 0);
        assert_eq!(history.backoff_until, 7000);
    }

    #[test]
    fn test_neighbor_demotion_after_failures() {
        let mut gossip = SbbGossip::new(8, 1000);
        let neighbor = create_test_neighbor(1);
        let peer_id = neighbor.peer_id.clone();

        gossip.add_active_neighbor(neighbor, 1000);
        assert_eq!(gossip.get_active_neighbor_count(), 1);

        gossip.record_merge_failure(&peer_id, 1000);
        assert_eq!(gossip.get_active_neighbor_count(), 1);

        gossip.record_merge_failure(&peer_id, 3000);
        assert_eq!(gossip.get_active_neighbor_count(), 1);

        gossip.record_merge_failure(&peer_id, 7000);
        assert_eq!(gossip.get_active_neighbor_count(), 0);
        assert_eq!(gossip.get_known_peer_count(), 1);
    }

    #[test]
    fn test_peer_promotion() {
        let mut gossip = SbbGossip::new(8, 1000);
        let peer_id = create_test_peer_id(1);
        let auth = create_test_auth(1, TrustLevel::Direct);

        gossip.add_known_peer(peer_id.clone(), auth);
        assert_eq!(gossip.get_known_peer_count(), 1);
        assert_eq!(gossip.get_active_neighbor_count(), 0);

        let result =
            gossip.promote_known_peer_to_neighbor(&peer_id, create_test_permissions(), 1000);
        assert!(result.is_ok());

        assert_eq!(gossip.get_known_peer_count(), 0);
        assert_eq!(gossip.get_active_neighbor_count(), 1);
    }

    #[test]
    fn test_max_active_neighbors() {
        let mut gossip = SbbGossip::new(2, 1000);

        gossip.add_active_neighbor(create_test_neighbor(1), 1000);
        gossip.add_active_neighbor(create_test_neighbor(2), 1000);
        assert_eq!(gossip.get_active_neighbor_count(), 2);

        let peer_id = create_test_peer_id(3);
        gossip.add_known_peer(peer_id.clone(), create_test_auth(3, TrustLevel::Direct));

        let result =
            gossip.promote_known_peer_to_neighbor(&peer_id, create_test_permissions(), 1000);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("No active neighbors"));
    }

    #[test]
    fn test_publish_envelope() {
        let mut gossip = SbbGossip::new(8, 1000);
        gossip.add_active_neighbor(create_test_neighbor(1), 1000);
        gossip.add_active_neighbor(create_test_neighbor(2), 1000);

        let envelope = create_test_envelope();
        let cid = Cid([1u8; 32]);

        let result = gossip.publish_envelope(cid.clone(), &envelope, 110, 1000);
        assert!(result.is_ok());

        let pushed_to = result.unwrap();
        assert_eq!(pushed_to.len(), 2);
        assert_eq!(gossip.get_envelope_count(), 1);
    }

    #[test]
    fn test_discover_peers_from_neighbor() {
        let mut gossip = SbbGossip::new(8, 1000);

        let neighbor_peers = vec![
            (
                create_test_peer_id(1),
                create_test_auth(1, TrustLevel::OneDegree),
            ),
            (
                create_test_peer_id(2),
                create_test_auth(2, TrustLevel::OneDegree),
            ),
        ];

        gossip.discover_peers_from_neighbor(neighbor_peers);
        assert_eq!(gossip.get_known_peer_count(), 2);
    }

    #[test]
    fn test_gc_expired_envelopes() {
        let mut gossip = SbbGossip::new(8, 1000);
        gossip.add_active_neighbor(create_test_neighbor(1), 1000);

        let envelope = create_test_envelope();

        let cid1 = Cid([1u8; 32]);
        let cid2 = Cid([2u8; 32]);

        gossip
            .publish_envelope(cid1.clone(), &envelope, 105, 1000)
            .unwrap();
        gossip
            .publish_envelope(cid2.clone(), &envelope, 110, 1000)
            .unwrap();

        assert_eq!(gossip.get_envelope_count(), 2);

        let expired = gossip.gc_expired_envelopes(106);
        assert_eq!(expired.len(), 1);
        assert_eq!(expired[0], cid1);
        assert_eq!(gossip.get_envelope_count(), 1);
    }

    #[test]
    fn test_eager_push_respects_rate_limits() {
        let mut gossip = SbbGossip::new(8, 1000);
        let neighbor = create_test_neighbor(1);
        let peer_id = neighbor.peer_id.clone();
        gossip.add_active_neighbor(neighbor, 1000);

        let envelope = create_test_envelope();
        let cid1 = Cid([1u8; 32]);

        let result = gossip.publish_envelope(cid1.clone(), &envelope, 110, 1000);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().len(), 1);

        gossip.record_merge_success(&peer_id, 1000);

        let cid2 = Cid([2u8; 32]);
        let result = gossip.publish_envelope(cid2.clone(), &envelope, 110, 1500);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().len(), 0);

        let cid3 = Cid([3u8; 32]);
        let result = gossip.publish_envelope(cid3.clone(), &envelope, 110, 2000);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().len(), 1);
    }
}
