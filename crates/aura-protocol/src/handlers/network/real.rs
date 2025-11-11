//! Real network handler using actual network transport
//!
//! Provides real network communication for production use.
//! Integrates with aura-transport layer and includes receipt verification.

use crate::effects::{NetworkEffects, NetworkError, PeerEventStream};
use async_trait::async_trait;
use aura_core::{DeviceId, Receipt};
use aura_transport::{NetworkMessage, NetworkTransport};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};
use uuid::Uuid;

/// Real network handler for production use with transport integration
pub struct RealNetworkHandler {
    device_id: DeviceId,
    transport: Arc<NetworkTransport>,
    uuid_to_device: Arc<RwLock<HashMap<Uuid, DeviceId>>>,
    device_to_uuid: Arc<RwLock<HashMap<DeviceId, Uuid>>>,
}

impl RealNetworkHandler {
    /// Create a new real network handler with transport integration
    pub fn new(transport: Arc<NetworkTransport>) -> Self {
        let device_id = DeviceId::from(transport.device_id().0.to_string());
        Self {
            device_id,
            transport,
            uuid_to_device: Arc::new(RwLock::new(HashMap::new())),
            device_to_uuid: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Register a UUID <-> DeviceId mapping for peer communication
    pub async fn register_peer(&self, uuid: Uuid, device_id: DeviceId) {
        let mut uuid_to_device = self.uuid_to_device.write().await;
        let mut device_to_uuid = self.device_to_uuid.write().await;
        uuid_to_device.insert(uuid, device_id);
        device_to_uuid.insert(device_id, uuid);
    }

    /// Send a message with receipt
    pub async fn send_with_receipt(
        &self,
        peer_uuid: Uuid,
        message: Vec<u8>,
        receipt: Option<Receipt>,
    ) -> Result<(), NetworkError> {
        let device_id = {
            let uuid_to_device = self.uuid_to_device.read().await;
            uuid_to_device.get(&peer_uuid).cloned()
        };

        if let Some(device_id) = device_id {
            self.transport
                .send_with_receipt(device_id, message, "data".to_string(), receipt)
                .await
                .map_err(|e| NetworkError::SendFailed(e.to_string()))
        } else {
            Err(NetworkError::ConnectionFailed(format!(
                "Unknown peer UUID: {}",
                peer_uuid
            )))
        }
    }

    /// Receive and verify a message
    pub async fn receive_verified(&self) -> Result<(Uuid, Vec<u8>, Option<Receipt>), NetworkError> {
        let message = self
            .transport
            .receive_verified()
            .await
            .map_err(|e| NetworkError::ReceiveFailed(e.to_string()))?;

        let peer_uuid = {
            let device_to_uuid = self.device_to_uuid.read().await;
            device_to_uuid.get(&message.from).cloned()
        };

        if let Some(uuid) = peer_uuid {
            Ok((uuid, message.payload, message.receipt))
        } else {
            // Auto-register unknown peers
            let uuid = Uuid::new_v4();
            self.register_peer(uuid, message.from).await;
            Ok((uuid, message.payload, message.receipt))
        }
    }
}

#[async_trait]
impl NetworkEffects for RealNetworkHandler {
    async fn send_to_peer(&self, peer_id: Uuid, message: Vec<u8>) -> Result<(), NetworkError> {
        self.send_with_receipt(peer_id, message, None).await
    }

    async fn broadcast(&self, message: Vec<u8>) -> Result<(), NetworkError> {
        let connected_peers = self.connected_peers().await;
        for peer_id in connected_peers {
            self.send_to_peer(peer_id, message.clone()).await?;
        }
        Ok(())
    }

    async fn receive(&self) -> Result<(Uuid, Vec<u8>), NetworkError> {
        let (uuid, payload, _receipt) = self.receive_verified().await?;
        Ok((uuid, payload))
    }

    async fn receive_from(&self, peer_id: Uuid) -> Result<Vec<u8>, NetworkError> {
        loop {
            let (sender_uuid, payload) = self.receive().await?;
            if sender_uuid == peer_id {
                return Ok(payload);
            }
            // TODO: Buffer messages from other peers instead of dropping
        }
    }

    async fn connected_peers(&self) -> Vec<Uuid> {
        let transport_peers = self.transport.connected_peers().await;
        let device_to_uuid = self.device_to_uuid.read().await;
        transport_peers
            .into_iter()
            .filter_map(|device_id| device_to_uuid.get(&device_id).cloned())
            .collect()
    }

    async fn is_peer_connected(&self, peer_id: Uuid) -> bool {
        let uuid_to_device = self.uuid_to_device.read().await;
        if let Some(device_id) = uuid_to_device.get(&peer_id) {
            self.transport.is_peer_connected(*device_id).await
        } else {
            false
        }
    }

    async fn subscribe_to_peer_events(&self) -> Result<PeerEventStream, NetworkError> {
        let (_sender, receiver) = mpsc::unbounded_channel();
        // TODO: Hook into transport layer peer events
        // This would monitor NetworkTransport for connection/disconnection events
        Ok(Box::pin(
            tokio_stream::wrappers::UnboundedReceiverStream::new(receiver),
        ))
    }
}
