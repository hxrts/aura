use std::collections::BTreeMap;

use crate::envelope::{Cid, Envelope};

pub type PeerId = Vec<u8>;
pub type AccountId = Vec<u8>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrustLevel {
    Direct,
    OneDegree,
    TwoDegree,
}

#[derive(Debug, Clone)]
pub struct PeerAuthentication {
    pub device_id: Vec<u8>,
    pub account_id: AccountId,
    pub last_authenticated: u64,
    pub trust_level: TrustLevel,
}

#[derive(Debug, Clone)]
pub struct PeerPermissions {
    pub relay_permissions: Vec<String>,
    pub communication_permissions: Vec<String>,
    pub storage_permissions: Vec<String>,
    pub granted_capabilities: Vec<Vec<u8>>,
    pub last_permission_update: u64,
}

#[derive(Debug, Clone)]
pub struct NeighborInfo {
    pub peer_id: PeerId,
    pub authentication: PeerAuthentication,
    pub permissions: PeerPermissions,
    pub added_at: u64,
}

#[derive(Debug, Clone)]
pub struct MergeHistory {
    pub last_merge_at: u64,
    pub merge_count: u64,
    pub consecutive_failures: u32,
    pub backoff_until: u64,
}

impl MergeHistory {
    pub fn new(now: u64) -> Self {
        Self {
            last_merge_at: 0,
            merge_count: 0,
            consecutive_failures: 0,
            backoff_until: 0,
        }
    }

    pub fn record_success(&mut self, now: u64) {
        self.last_merge_at = now;
        self.merge_count += 1;
        self.consecutive_failures = 0;
        self.backoff_until = now;
    }

    pub fn record_failure(&mut self, now: u64) {
        self.consecutive_failures += 1;
        let backoff_ms = 1000u64 * 2u64.pow(self.consecutive_failures.min(10));
        self.backoff_until = now + backoff_ms;
    }

    pub fn can_merge(&self, now: u64) -> bool {
        now >= self.backoff_until
    }
}

pub struct RateLimiter {
    min_merge_interval_ms: u64,
}

impl RateLimiter {
    pub fn new(min_merge_interval_ms: u64) -> Self {
        Self {
            min_merge_interval_ms,
        }
    }

    pub fn check_rate_limit(&self, history: &MergeHistory, now: u64) -> bool {
        if !history.can_merge(now) {
            return false;
        }
        let elapsed = now.saturating_sub(history.last_merge_at);
        elapsed >= self.min_merge_interval_ms
    }
}

#[derive(Debug, Clone)]
pub struct EnvelopeMetadata {
    pub cid: Cid,
    pub added_at: u64,
    pub expires_at_epoch: u64,
    pub size_bytes: usize,
}

pub struct SbbGossip {
    active_neighbors: BTreeMap<PeerId, NeighborInfo>,
    known_peers: BTreeMap<PeerId, PeerAuthentication>,
    merge_history: BTreeMap<PeerId, MergeHistory>,
    local_envelopes: BTreeMap<Cid, EnvelopeMetadata>,
    rate_limiter: RateLimiter,
    max_active_neighbors: usize,
}

#[derive(Debug)]
pub enum GossipError {
    RateLimitExceeded,
    UnknownPeer,
    BackoffActive,
    NoActiveNeighbors,
    InvalidEnvelope,
}

pub type Result<T> = std::result::Result<T, GossipError>;

impl SbbGossip {
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

    pub fn add_active_neighbor(&mut self, neighbor: NeighborInfo, now: u64) {
        let peer_id = neighbor.peer_id.clone();
        self.active_neighbors.insert(peer_id.clone(), neighbor);
        self.merge_history
            .entry(peer_id)
            .or_insert_with(|| MergeHistory::new(now));
    }

    pub fn remove_active_neighbor(&mut self, peer_id: &PeerId) {
        self.active_neighbors.remove(peer_id);
    }

    pub fn add_known_peer(&mut self, peer_id: PeerId, auth: PeerAuthentication) {
        self.known_peers.insert(peer_id, auth);
    }

    pub fn publish_envelope(
        &mut self,
        cid: Cid,
        envelope: &Envelope,
        expires_at_epoch: u64,
        now: u64,
    ) -> Result<Vec<PeerId>> {
        let metadata = EnvelopeMetadata {
            cid: cid.clone(),
            added_at: now,
            expires_at_epoch,
            size_bytes: 2048,
        };
        self.local_envelopes.insert(cid.clone(), metadata);

        self.eager_push_to_neighbors(now)
    }

    fn eager_push_to_neighbors(&mut self, now: u64) -> Result<Vec<PeerId>> {
        if self.active_neighbors.is_empty() {
            return Err(GossipError::NoActiveNeighbors);
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

    pub fn initiate_merge_with_neighbor(&mut self, peer_id: &PeerId, now: u64) -> Result<()> {
        if !self.active_neighbors.contains_key(peer_id) {
            return Err(GossipError::UnknownPeer);
        }

        let history = self
            .merge_history
            .get(peer_id)
            .ok_or(GossipError::UnknownPeer)?;

        if !self.rate_limiter.check_rate_limit(history, now) {
            return Err(GossipError::RateLimitExceeded);
        }

        if !history.can_merge(now) {
            return Err(GossipError::BackoffActive);
        }

        Ok(())
    }

    pub fn record_merge_success(&mut self, peer_id: &PeerId, now: u64) {
        self.merge_history
            .entry(peer_id.clone())
            .or_insert_with(|| MergeHistory::new(now))
            .record_success(now);
    }

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

    pub fn promote_known_peer_to_neighbor(
        &mut self,
        peer_id: &PeerId,
        permissions: PeerPermissions,
        now: u64,
    ) -> Result<()> {
        if self.active_neighbors.len() >= self.max_active_neighbors {
            return Err(GossipError::NoActiveNeighbors);
        }

        let auth = self
            .known_peers
            .get(peer_id)
            .ok_or(GossipError::UnknownPeer)?
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

    pub fn get_active_neighbor_count(&self) -> usize {
        self.active_neighbors.len()
    }

    pub fn get_known_peer_count(&self) -> usize {
        self.known_peers.len()
    }

    pub fn get_envelope_count(&self) -> usize {
        self.local_envelopes.len()
    }

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
        use crate::envelope::{Header, HeaderBare, RoutingTag};
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
        assert!(matches!(result, Err(GossipError::RateLimitExceeded)));

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
        assert!(matches!(result, Err(GossipError::NoActiveNeighbors)));
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
