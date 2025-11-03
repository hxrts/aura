//! In-memory network handler for testing
//!
//! Provides a simple in-memory message passing system for unit tests and local development.

use crate::effects::{NetworkEffects, NetworkError, PeerEvent, PeerEventStream};
use async_trait::async_trait;
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex, RwLock};
use uuid::Uuid;

/// In-memory network handler for testing
pub struct MemoryNetworkHandler {
    device_id: Uuid,
    peers: Arc<RwLock<HashMap<Uuid, PeerConnection>>>,
    messages: Arc<Mutex<VecDeque<(Uuid, Vec<u8>)>>>,
    event_sender: Arc<Mutex<Option<mpsc::UnboundedSender<PeerEvent>>>>,
}

struct PeerConnection {
    id: Uuid,
    connected: bool,
    message_queue: VecDeque<Vec<u8>>,
}

impl MemoryNetworkHandler {
    /// Create a new in-memory network handler
    pub fn new(device_id: Uuid) -> Self {
        Self {
            device_id,
            peers: Arc::new(RwLock::new(HashMap::new())),
            messages: Arc::new(Mutex::new(VecDeque::new())),
            event_sender: Arc::new(Mutex::new(None)),
        }
    }

    /// Add a peer to the network
    pub async fn add_peer(&self, peer_id: Uuid) {
        let mut peers = self.peers.write().await;
        peers.insert(
            peer_id,
            PeerConnection {
                id: peer_id,
                connected: true,
                message_queue: VecDeque::new(),
            },
        );

        // Notify of peer connection
        if let Some(sender) = &*self.event_sender.lock().await {
            let _ = sender.send(PeerEvent::Connected { peer_id });
        }
    }

    /// Remove a peer from the network
    pub async fn remove_peer(&self, peer_id: Uuid) {
        let mut peers = self.peers.write().await;
        if peers.remove(&peer_id).is_some() {
            // Notify of peer disconnection
            if let Some(sender) = &*self.event_sender.lock().await {
                let _ = sender.send(PeerEvent::Disconnected { peer_id });
            }
        }
    }

    /// Deliver a message to this handler from another peer
    pub async fn deliver_message(&self, from_peer: Uuid, message: Vec<u8>) {
        let mut messages = self.messages.lock().await;
        messages.push_back((from_peer, message));
    }
}

#[async_trait]
impl NetworkEffects for MemoryNetworkHandler {
    async fn send_to_peer(&self, peer_id: Uuid, _message: Vec<u8>) -> Result<(), NetworkError> {
        let peers = self.peers.read().await;
        if let Some(peer) = peers.get(&peer_id) {
            if peer.connected {
                // In a real implementation, this would send to the actual peer
                // For testing, we just simulate success
                Ok(())
            } else {
                Err(NetworkError::PeerNotConnected { peer_id })
            }
        } else {
            Err(NetworkError::PeerNotConnected { peer_id })
        }
    }

    async fn broadcast(&self, message: Vec<u8>) -> Result<(), NetworkError> {
        let peers = self.peers.read().await;
        for (peer_id, peer) in peers.iter() {
            if peer.connected {
                // Simulate sending to each connected peer
                self.send_to_peer(*peer_id, message.clone()).await?;
            }
        }
        Ok(())
    }

    async fn receive(&self) -> Result<(Uuid, Vec<u8>), NetworkError> {
        let mut messages = self.messages.lock().await;
        if let Some((from_peer, message)) = messages.pop_front() {
            Ok((from_peer, message))
        } else {
            // In a real implementation, this would block until a message arrives
            // For testing, we return a timeout error
            Err(NetworkError::ReceiveTimeout { timeout_ms: 1000 })
        }
    }

    async fn receive_from(&self, peer_id: Uuid) -> Result<Vec<u8>, NetworkError> {
        let mut messages = self.messages.lock().await;

        // Find message from specific peer
        if let Some(pos) = messages.iter().position(|(from, _)| *from == peer_id) {
            let (_, message) = messages.remove(pos).unwrap();
            Ok(message)
        } else {
            Err(NetworkError::ReceiveTimeout { timeout_ms: 1000 })
        }
    }

    async fn connected_peers(&self) -> Vec<Uuid> {
        let peers = self.peers.read().await;
        peers
            .values()
            .filter(|peer| peer.connected)
            .map(|peer| peer.id)
            .collect()
    }

    async fn is_peer_connected(&self, peer_id: Uuid) -> bool {
        let peers = self.peers.read().await;
        peers
            .get(&peer_id)
            .map(|peer| peer.connected)
            .unwrap_or(false)
    }

    async fn subscribe_to_peer_events(&self) -> Result<PeerEventStream, NetworkError> {
        let (sender, receiver) = mpsc::unbounded_channel();
        *self.event_sender.lock().await = Some(sender);

        Ok(Box::new(
            tokio_stream::wrappers::UnboundedReceiverStream::new(receiver),
        ))
    }
}
