//! In-memory network handler for testing
//!
//! Provides a simple in-memory message passing system for unit tests and local development.

use crate::effects::{NetworkEffects, NetworkError, PeerEvent, PeerEventStream};
use async_trait::async_trait;
use once_cell::sync::Lazy;
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex, Weak};
use tokio::sync::mpsc;
use tokio_stream::wrappers::UnboundedReceiverStream;
use uuid::Uuid;

static MEMORY_NETWORK_REGISTRY: Lazy<Mutex<HashMap<Uuid, Weak<MemoryNetworkState>>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

struct MemoryNetworkState {
    device_id: Uuid,
    inbox: Mutex<VecDeque<(Uuid, Vec<u8>)>>,
    event_sender: Mutex<Option<mpsc::UnboundedSender<PeerEvent>>>,
}

impl MemoryNetworkState {
    fn new(device_id: Uuid) -> Self {
        Self {
            device_id,
            inbox: Mutex::new(VecDeque::new()),
            event_sender: Mutex::new(None),
        }
    }

    fn push_message(&self, from: Uuid, message: Vec<u8>) {
        let mut inbox = self.inbox.lock().unwrap();
        inbox.push_back((from, message));
    }

    fn pop_message(&self) -> Option<(Uuid, Vec<u8>)> {
        let mut inbox = self.inbox.lock().unwrap();
        inbox.pop_front()
    }

    fn pop_message_from(&self, peer_id: Uuid) -> Option<Vec<u8>> {
        let mut inbox = self.inbox.lock().unwrap();
        if let Some(pos) = inbox.iter().position(|(from, _)| *from == peer_id) {
            inbox.remove(pos).map(|(_, msg)| msg)
        } else {
            None
        }
    }

    fn set_event_sender(&self, sender: mpsc::UnboundedSender<PeerEvent>) {
        let mut guard = self.event_sender.lock().unwrap();
        *guard = Some(sender);
    }

    fn send_event(&self, event: PeerEvent) {
        if let Some(sender) = &*self.event_sender.lock().unwrap() {
            let _ = sender.send(event);
        }
    }
}

/// In-memory network handler for testing
pub struct MemoryNetworkHandler {
    device_id: Uuid,
    state: Arc<MemoryNetworkState>,
}

impl MemoryNetworkHandler {
    /// Create a new in-memory network handler
    pub fn new(device_id: Uuid) -> Self {
        let state = Arc::new(MemoryNetworkState::new(device_id));
        MEMORY_NETWORK_REGISTRY
            .lock()
            .unwrap()
            .insert(device_id, Arc::downgrade(&state));

        Self { device_id, state }
    }

    /// Add a peer to the network
    pub async fn add_peer(&self, peer_id: Uuid) {
        if let Some(state) = Self::lookup_peer(peer_id) {
            state.send_event(PeerEvent::Connected(self.device_id));
        }
        self.state.send_event(PeerEvent::Connected(peer_id));
    }

    /// Remove a peer from the network
    pub async fn remove_peer(&self, peer_id: Uuid) {
        self.state.send_event(PeerEvent::Disconnected(peer_id));
        if let Some(state) = Self::lookup_peer(peer_id) {
            state.send_event(PeerEvent::Disconnected(self.device_id));
        }
    }

    fn lookup_peer(peer_id: Uuid) -> Option<Arc<MemoryNetworkState>> {
        MEMORY_NETWORK_REGISTRY
            .lock()
            .unwrap()
            .get(&peer_id)
            .and_then(|weak| weak.upgrade())
    }
}

impl Drop for MemoryNetworkHandler {
    fn drop(&mut self) {
        MEMORY_NETWORK_REGISTRY
            .lock()
            .unwrap()
            .remove(&self.device_id);
    }
}

#[async_trait]
impl NetworkEffects for MemoryNetworkHandler {
    async fn send_to_peer(&self, peer_id: Uuid, message: Vec<u8>) -> Result<(), NetworkError> {
        let target_state = Self::lookup_peer(peer_id).ok_or_else(|| {
            NetworkError::ConnectionFailed(format!("Peer not registered: {}", peer_id))
        })?;

        target_state.push_message(self.device_id, message);
        target_state.send_event(PeerEvent::Connected(self.device_id));
        Ok(())
    }

    async fn broadcast(&self, message: Vec<u8>) -> Result<(), NetworkError> {
        let peers: Vec<Uuid> = MEMORY_NETWORK_REGISTRY
            .lock()
            .unwrap()
            .keys()
            .copied()
            .filter(|id| *id != self.device_id)
            .collect();

        for peer_id in peers {
            self.send_to_peer(peer_id, message.clone()).await?;
        }
        Ok(())
    }

    async fn receive(&self) -> Result<(Uuid, Vec<u8>), NetworkError> {
        self.state.pop_message().ok_or(NetworkError::NoMessage)
    }

    async fn receive_from(&self, peer_id: Uuid) -> Result<Vec<u8>, NetworkError> {
        self.state
            .pop_message_from(peer_id)
            .ok_or(NetworkError::NoMessage)
    }

    async fn connected_peers(&self) -> Vec<Uuid> {
        MEMORY_NETWORK_REGISTRY
            .lock()
            .unwrap()
            .keys()
            .copied()
            .filter(|id| *id != self.device_id)
            .collect()
    }

    async fn is_peer_connected(&self, peer_id: Uuid) -> bool {
        MEMORY_NETWORK_REGISTRY
            .lock()
            .unwrap()
            .contains_key(&peer_id)
    }

    async fn subscribe_to_peer_events(&self) -> Result<PeerEventStream, NetworkError> {
        let (sender, receiver) = mpsc::unbounded_channel();
        self.state.set_event_sender(sender);

        Ok(Box::pin(
            tokio_stream::wrappers::UnboundedReceiverStream::new(receiver),
        ))
    }
}
