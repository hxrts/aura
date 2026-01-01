#![allow(missing_docs)]
#![allow(clippy::type_complexity)]
use async_trait::async_trait;
use aura_core::effects::JournalEffects;
use aura_core::effects::{NetworkCoreEffects, NetworkError, NetworkExtendedEffects};
use aura_core::effects::{PhysicalTimeEffects, TimeError};
use aura_core::identifiers::{AuthorityId, ContextId, DeviceId};
use aura_core::time::PhysicalTime;
use aura_core::{FlowBudget, Journal};
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
    fn new(
        id: Uuid,
        inbox: PeerReceiver,
        initial_journal: Journal,
    ) -> Self {
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
        cost: u32,
    ) -> Result<FlowBudget, aura_core::AuraError> {
        Ok(FlowBudget {
            limit: 1000,
            spent: cost as u64,
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

#[tokio::test]
async fn journal_sync_two_peers_is_noop_on_second_run() {
    let (tx_a, rx_a) = mpsc::unbounded_channel();
    let (tx_b, rx_b) = mpsc::unbounded_channel();

    let id_a = Uuid::from_bytes([1u8; 16]);
    let id_b = Uuid::from_bytes([2u8; 16]);

    let effects_a = TestEffects::new(id_a, rx_a, Journal::new());
    let effects_b = TestEffects::new(id_b, rx_b, Journal::new());

    effects_a.add_peer(id_b, tx_b.clone()).await;
    effects_b.add_peer(id_a, tx_a.clone()).await;

    let mut protocol_a = JournalSyncProtocol::new(JournalSyncConfig::default());
    let mut protocol_b = JournalSyncProtocol::new(JournalSyncConfig::default());

    let (first_a, first_b) = tokio::join!(
        protocol_a.sync_with_peer(&effects_a, DeviceId(id_b)),
        protocol_b.sync_with_peer(&effects_b, DeviceId(id_a)),
    );

    assert_eq!(
        first_a.unwrap_or_else(|e| panic!("sync A succeeds: {e}")),
        0
    );
    assert_eq!(
        first_b.unwrap_or_else(|e| panic!("sync B succeeds: {e}")),
        0
    );

    let (second_a, second_b) = tokio::join!(
        protocol_a.sync_with_peer(&effects_a, DeviceId(id_b)),
        protocol_b.sync_with_peer(&effects_b, DeviceId(id_a)),
    );

    assert_eq!(
        second_a.unwrap_or_else(|e| panic!("sync A succeeds: {e}")),
        0
    );
    assert_eq!(
        second_b.unwrap_or_else(|e| panic!("sync B succeeds: {e}")),
        0
    );
}
