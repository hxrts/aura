//! Unified in-memory transport implementation

use crate::{
    core::traits::Transport, error::TransportErrorBuilder, TransportEnvelope, TransportResult,
};
use async_trait::async_trait;
use aura_types::DeviceId;
use std::{collections::HashMap, sync::Arc, time::Duration};
use tokio::sync::{mpsc, Mutex, RwLock};

/// In-memory transport for testing and local communication
#[derive(Clone)]
pub struct MemoryTransport {
    device_id: DeviceId,
    peers: Arc<RwLock<HashMap<DeviceId, Arc<MemoryTransport>>>>,
    receiver: Arc<Mutex<mpsc::UnboundedReceiver<TransportEnvelope>>>,
    sender: mpsc::UnboundedSender<TransportEnvelope>,
}

impl MemoryTransport {
    /// Create a new memory transport
    pub fn new(device_id: DeviceId) -> Self {
        let (sender, receiver) = mpsc::unbounded_channel();
        Self {
            device_id,
            peers: Arc::new(RwLock::new(HashMap::new())),
            receiver: Arc::new(Mutex::new(receiver)),
            sender,
        }
    }

    /// Connect two memory transports together
    pub async fn connect_transports(transport1: &Self, transport2: &Self) {
        transport1
            .peers
            .write()
            .await
            .insert(transport2.device_id, Arc::new(transport2.clone()));
        transport2
            .peers
            .write()
            .await
            .insert(transport1.device_id, Arc::new(transport1.clone()));
    }
}

#[async_trait]
impl Transport for MemoryTransport {
    async fn send(&self, envelope: TransportEnvelope) -> TransportResult<()> {
        let peers = self.peers.read().await;
        if let Some(peer) = peers.get(&envelope.to) {
            peer.sender
                .send(envelope)
                .map_err(|_| TransportErrorBuilder::connection("Peer disconnected"))?;
            Ok(())
        } else {
            Err(TransportErrorBuilder::peer_unreachable(
                envelope.to.to_string(),
            ))
        }
    }

    async fn receive(&self, timeout: Duration) -> TransportResult<Option<TransportEnvelope>> {
        let mut receiver = self.receiver.lock().await;
        tokio::time::timeout(timeout, receiver.recv())
            .await
            .map_err(|_| TransportErrorBuilder::timeout("receive"))
            .map(|opt| opt.map(Some).unwrap_or(None))
    }

    async fn connect(&self, peer_id: DeviceId) -> TransportResult<()> {
        // Memory transport doesn't need explicit connection
        if !self.peers.read().await.contains_key(&peer_id) {
            return Err(TransportErrorBuilder::peer_unreachable(peer_id.to_string()));
        }
        Ok(())
    }

    async fn disconnect(&self, peer_id: DeviceId) -> TransportResult<()> {
        self.peers.write().await.remove(&peer_id);
        Ok(())
    }

    async fn is_reachable(&self, peer_id: DeviceId) -> bool {
        self.peers.read().await.contains_key(&peer_id)
    }

    async fn start(&mut self) -> TransportResult<()> {
        // Memory transport is always ready
        Ok(())
    }

    async fn stop(&mut self) -> TransportResult<()> {
        self.peers.write().await.clear();
        Ok(())
    }

    fn transport_type(&self) -> &'static str {
        "memory"
    }
}
