#![allow(missing_docs)]
#![allow(clippy::type_complexity)]
use async_trait::async_trait;
use aura_core::effects::JournalEffects;
use aura_core::effects::{NetworkCoreEffects, NetworkError, NetworkExtendedEffects};
use aura_core::effects::{PhysicalTimeEffects, TimeError};
use aura_core::identifiers::{AuthorityId, ContextId, DeviceId};
use aura_core::time::PhysicalTime;
use aura_core::{FlowBudget, FlowCost, Journal};
use aura_sync::protocols::{JournalSyncConfig, JournalSyncProtocol};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};
use uuid::Uuid;

type PeerMessage = (Uuid, Vec<u8>);
type PeerSender = mpsc::UnboundedSender<PeerMessage>;
type PeerReceiver = mpsc::UnboundedReceiver<PeerMessage>;
type PeerMap = HashMap<Uuid, PeerSender>;

#[derive(Clone)]
struct TestEffects {
    id: Uuid,
    journal: Arc<Mutex<Journal>>,
    peers: Arc<Mutex<PeerMap>>,
    inbox: Arc<Mutex<PeerReceiver>>,
    time_ms: Arc<AtomicU64>,
}

impl TestEffects {
    fn new(id: Uuid, inbox: PeerReceiver, initial_journal: Journal) -> Self {
        Self {
            id,
            journal: Arc::new(Mutex::new(initial_journal)),
            peers: Arc::new(Mutex::new(HashMap::new())),
            inbox: Arc::new(Mutex::new(inbox)),
            time_ms: Arc::new(AtomicU64::new(0)),
        }
    }

    async fn add_peer(&self, peer_id: Uuid, sender: PeerSender) {
        self.peers.lock().await.insert(peer_id, sender);
    }
}

#[async_trait]
impl JournalEffects for TestEffects {
    async fn merge_facts(
        &self,
        target: &Journal,
        delta: &Journal,
    ) -> Result<Journal, aura_core::AuraError> {
        let mut merged = target.clone();
        merged.merge_facts(delta.facts.clone());
        Ok(merged)
    }

    async fn refine_caps(
        &self,
        target: &Journal,
        refinement: &Journal,
    ) -> Result<Journal, aura_core::AuraError> {
        let mut refined = target.clone();
        refined.refine_caps(refinement.caps.clone());
        Ok(refined)
    }

    async fn get_journal(&self) -> Result<Journal, aura_core::AuraError> {
        Ok(self.journal.lock().await.clone())
    }

    async fn persist_journal(&self, journal: &Journal) -> Result<(), aura_core::AuraError> {
        *self.journal.lock().await = journal.clone();
        Ok(())
    }

    async fn get_flow_budget(
        &self,
        _context: &ContextId,
        _peer: &AuthorityId,
    ) -> Result<FlowBudget, aura_core::AuraError> {
        Ok(FlowBudget {
            limit: 1000,
            spent: 0,
            epoch: aura_core::types::Epoch::new(0),
        })
    }

    async fn update_flow_budget(
        &self,
        _context: &ContextId,
        _peer: &AuthorityId,
        budget: &FlowBudget,
    ) -> Result<FlowBudget, aura_core::AuraError> {
        Ok(*budget)
    }

    async fn charge_flow_budget(
        &self,
        _context: &ContextId,
        _peer: &AuthorityId,
        cost: FlowCost,
    ) -> Result<FlowBudget, aura_core::AuraError> {
        Ok(FlowBudget {
            limit: 1000,
            spent: u64::from(cost),
            epoch: aura_core::types::Epoch::new(0),
        })
    }
}

#[async_trait]
impl NetworkCoreEffects for TestEffects {
    async fn send_to_peer(&self, peer_id: Uuid, message: Vec<u8>) -> Result<(), NetworkError> {
        let sender = self
            .peers
            .lock()
            .await
            .get(&peer_id)
            .cloned()
            .ok_or_else(|| NetworkError::SendFailed {
                peer_id: Some(peer_id),
                reason: "unknown peer".to_string(),
            })?;
        sender
            .send((self.id, message))
            .map_err(|_| NetworkError::SendFailed {
                peer_id: Some(peer_id),
                reason: "channel closed".to_string(),
            })
    }

    async fn broadcast(&self, message: Vec<u8>) -> Result<(), NetworkError> {
        let peers = self.peers.lock().await.clone();
        for (peer_id, sender) in peers {
            sender
                .send((self.id, message.clone()))
                .map_err(|_| NetworkError::SendFailed {
                    peer_id: Some(peer_id),
                    reason: "channel closed".to_string(),
                })?;
        }
        Ok(())
    }

    async fn receive(&self) -> Result<(Uuid, Vec<u8>), NetworkError> {
        let mut inbox = self.inbox.lock().await;
        inbox.recv().await.ok_or(NetworkError::NoMessage)
    }
}

#[async_trait]
impl NetworkExtendedEffects for TestEffects {}

#[async_trait]
impl PhysicalTimeEffects for TestEffects {
    async fn physical_time(&self) -> Result<PhysicalTime, TimeError> {
        Ok(PhysicalTime {
            ts_ms: self.time_ms.load(Ordering::SeqCst),
            uncertainty: None,
        })
    }

    async fn sleep_ms(&self, ms: u64) -> Result<(), TimeError> {
        self.time_ms.fetch_add(ms, Ordering::SeqCst);
        Ok(())
    }
}

/// Test that sync fails without proper Biscuit authorization.
///
/// This validates that the authorization check is enforced - sync operations
/// require proper Biscuit token configuration.
///
/// NOTE: Full Biscuit-authorized sync tests are complex and require:
/// 1. TokenAuthority setup with matching keypairs
/// 2. BiscuitGuardEvaluator configured with the same public key
/// 3. Tokens with appropriate capability facts
///
/// The integration is designed for production use where tokens are issued
/// by a central authority. Unit tests should use mock authorization or
/// the agent-level test infrastructure which handles the full token lifecycle.
#[tokio::test]
async fn journal_sync_requires_authorization() {
    let (tx_a, rx_a) = mpsc::unbounded_channel::<PeerMessage>();
    let (_tx_b, _rx_b) = mpsc::unbounded_channel::<PeerMessage>();

    let id_a = Uuid::from_bytes([1u8; 16]);
    let id_b = Uuid::from_bytes([2u8; 16]);

    let effects_a = TestEffects::new(id_a, rx_a, Journal::new());
    effects_a.add_peer(id_b, tx_a.clone()).await;

    // Create protocol WITHOUT Biscuit authorization
    let mut protocol = JournalSyncProtocol::new(JournalSyncConfig::default());

    // Sync should fail because authorization is required but not configured
    let result = protocol.sync_with_peer(&effects_a, DeviceId(id_b)).await;

    assert!(
        result.is_err(),
        "Sync should fail without authorization configuration"
    );

    let error = result.unwrap_err();
    let error_str = error.to_string();
    assert!(
        error_str.contains("Authorization required")
            || error_str.contains("permission")
            || error_str.contains("denied"),
        "Error should indicate authorization failure, got: {error_str}"
    );
}

/// Test that protocol creation works with default config.
#[test]
fn journal_sync_protocol_creation() {
    let config = JournalSyncConfig::default();
    let protocol = JournalSyncProtocol::new(config);

    let stats = protocol.statistics();
    assert_eq!(stats.total_peers, 0);
    assert_eq!(stats.syncing_peers, 0);
    assert_eq!(stats.synced_peers, 0);
    assert_eq!(stats.failed_peers, 0);
}

/// Test peer state tracking works correctly.
#[test]
fn journal_sync_peer_state_tracking() {
    use aura_sync::protocols::journal::SyncState;

    let mut protocol = JournalSyncProtocol::default();
    let peer = DeviceId::from_bytes([1; 32]);

    // Initially no state
    assert!(protocol.get_peer_state(&peer).is_none());

    // Set to syncing
    protocol.update_peer_state(peer, SyncState::Syncing);
    assert!(matches!(
        protocol.get_peer_state(&peer),
        Some(SyncState::Syncing)
    ));

    // Set to synced
    protocol.update_peer_state(peer, SyncState::synced_from_ms(1000, 42));
    match protocol.get_peer_state(&peer) {
        Some(SyncState::Synced { operations, .. }) => {
            assert_eq!(*operations, 42);
        }
        _ => panic!("Expected Synced state"),
    }

    // Check statistics
    let stats = protocol.statistics();
    assert_eq!(stats.total_peers, 1);
    assert_eq!(stats.synced_peers, 1);

    // Clear states
    protocol.clear_states();
    assert!(protocol.get_peer_state(&peer).is_none());
}

/// Test peer state pruning removes stale entries.
#[test]
fn journal_sync_peer_state_pruning() {
    use aura_sync::protocols::journal::SyncState;

    let mut protocol = JournalSyncProtocol::default();

    // Add several peers with different timestamps
    for i in 0..10u8 {
        let peer = DeviceId::from_bytes([i; 32]);
        let ts = (i as u64) * 1000; // 0, 1000, 2000, ...
        protocol.update_peer_state(peer, SyncState::synced_from_ms(ts, 1));
    }

    assert_eq!(protocol.statistics().total_peers, 10);

    // Prune entries older than 5000ms, keeping max 5 peers
    let now_ms = 10_000;
    let stale_ms = 5_000;
    let max_peers = 5;

    let pruned = protocol.prune_peer_states(now_ms, stale_ms, max_peers);

    // Should have removed at least 5 (all entries older than 5000ms: 0, 1000, 2000, 3000, 4000)
    assert!(pruned >= 5, "Expected at least 5 pruned, got {pruned}");
    assert!(
        protocol.statistics().total_peers <= max_peers as u64,
        "Should have at most {} peers",
        max_peers
    );
}
