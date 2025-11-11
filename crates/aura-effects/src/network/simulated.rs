//! Simulated network handler for deterministic testing
//!
//! Provides controllable network behavior for testing protocol resilience.

use aura_core::effects::{NetworkEffects, NetworkError, PeerEvent, PeerEventStream};
use async_trait::async_trait;
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex, RwLock};
use uuid::Uuid;

/// Type alias for complex message queue to reduce type complexity
type GlobalMessageQueue = Arc<Mutex<VecDeque<(Uuid, Uuid, Vec<u8>)>>>;

/// Simulated network handler with controllable behavior
pub struct SimulatedNetworkHandler {
    device_id: Uuid,
    peers: Arc<RwLock<HashMap<Uuid, SimulatedPeer>>>,
    global_message_queue: GlobalMessageQueue, // (from, to, message)
    network_conditions: Arc<RwLock<NetworkConditions>>,
    event_sender: Arc<Mutex<Option<mpsc::UnboundedSender<PeerEvent>>>>,
}

#[derive(Debug, Clone)]
struct SimulatedPeer {
    id: Uuid,
    connected: bool,
    message_queue: VecDeque<Vec<u8>>,
    latency_ms: u64,
    drop_rate: f64, // 0.0 to 1.0
}

#[derive(Debug, Clone)]
/// Network simulation conditions for testing
pub struct NetworkConditions {
    /// Base latency in milliseconds
    pub base_latency_ms: u64,
    /// Latency variance (jitter) in milliseconds
    pub latency_variance_ms: u64,
    /// Packet drop rate (0.0 to 1.0)
    pub drop_rate: f64,
    /// Network partition - if true, all messages are dropped
    pub partitioned: bool,
    /// Bandwidth limit in bytes per second (None = unlimited)
    pub bandwidth_limit: Option<u64>,
}

impl Default for NetworkConditions {
    fn default() -> Self {
        Self {
            base_latency_ms: 10,
            latency_variance_ms: 5,
            drop_rate: 0.0,
            partitioned: false,
            bandwidth_limit: None,
        }
    }
}

impl SimulatedNetworkHandler {
    /// Create a new simulated network handler
    pub fn new(device_id: Uuid) -> Self {
        Self {
            device_id,
            peers: Arc::new(RwLock::new(HashMap::new())),
            global_message_queue: Arc::new(Mutex::new(VecDeque::new())),
            network_conditions: Arc::new(RwLock::new(NetworkConditions::default())),
            event_sender: Arc::new(Mutex::new(None)),
        }
    }

    /// Add a simulated peer
    pub async fn add_peer(&self, peer_id: Uuid) {
        let mut peers = self.peers.write().await;
        peers.insert(
            peer_id,
            SimulatedPeer {
                id: peer_id,
                connected: true,
                message_queue: VecDeque::new(),
                latency_ms: 10,
                drop_rate: 0.0,
            },
        );

        if let Some(sender) = &*self.event_sender.lock().await {
            let _ = sender.send(PeerEvent::Connected(peer_id));
        }
    }

    /// Remove a simulated peer
    pub async fn remove_peer(&self, peer_id: Uuid) {
        let mut peers = self.peers.write().await;
        if peers.remove(&peer_id).is_some() {
            if let Some(sender) = &*self.event_sender.lock().await {
                let _ = sender.send(PeerEvent::Disconnected(peer_id));
            }
        }
    }

    /// Update network conditions for testing
    pub async fn set_network_conditions(&self, conditions: NetworkConditions) {
        *self.network_conditions.write().await = conditions;
    }

    /// Simulate network partition
    pub async fn set_partitioned(&self, partitioned: bool) {
        self.network_conditions.write().await.partitioned = partitioned;
    }

    /// Simulate message drops
    pub async fn set_drop_rate(&self, drop_rate: f64) {
        self.network_conditions.write().await.drop_rate = drop_rate;
    }

    /// Deliver a message with simulated network conditions
    async fn simulate_message_delivery(&self, from: Uuid, to: Uuid, message: Vec<u8>) -> bool {
        let conditions = self.network_conditions.read().await;

        // Check if network is partitioned
        if conditions.partitioned {
            return false;
        }

        // Simulate message drop
        if conditions.drop_rate > 0.0 {
            use rand::Rng;
            #[allow(clippy::disallowed_methods)] // Needed for network simulation
            let mut rng = rand::thread_rng();
            if rng.gen::<f64>() < conditions.drop_rate {
                return false;
            }
        }

        // Simulate latency (in a real simulation, this would involve time control)
        let _total_latency = conditions.base_latency_ms
            + if conditions.latency_variance_ms > 0 {
                use rand::Rng;
                #[allow(clippy::disallowed_methods)] // Needed for network simulation
                let mut rng = rand::thread_rng();
                rng.gen_range(0..=conditions.latency_variance_ms)
            } else {
                0
            };

        // Add to global message queue
        self.global_message_queue
            .lock()
            .await
            .push_back((from, to, message));
        true
    }
}

#[async_trait]
impl NetworkEffects for SimulatedNetworkHandler {
    async fn send_to_peer(&self, peer_id: Uuid, message: Vec<u8>) -> Result<(), NetworkError> {
        let peers = self.peers.read().await;
        if let Some(peer) = peers.get(&peer_id) {
            if peer.connected {
                // Simulate network conditions
                let delivered = self
                    .simulate_message_delivery(self.device_id, peer_id, message)
                    .await;
                if delivered {
                    Ok(())
                } else {
                    // Message was dropped due to network conditions
                    Err(NetworkError::SendFailed(
                        "Message dropped due to network conditions".to_string(),
                    ))
                }
            } else {
                Err(NetworkError::ConnectionFailed(format!(
                    "Peer not connected: {}",
                    peer_id
                )))
            }
        } else {
            Err(NetworkError::ConnectionFailed(format!(
                "Peer not connected: {}",
                peer_id
            )))
        }
    }

    async fn broadcast(&self, message: Vec<u8>) -> Result<(), NetworkError> {
        let peers = self.peers.read().await;
        for (peer_id, peer) in peers.iter() {
            if peer.connected {
                // Don't fail entire broadcast if one peer is unreachable
                let _ = self.send_to_peer(*peer_id, message.clone()).await;
            }
        }
        Ok(())
    }

    async fn receive(&self) -> Result<(Uuid, Vec<u8>), NetworkError> {
        let mut queue = self.global_message_queue.lock().await;

        // Find any message for this device
        if let Some(pos) = queue.iter().position(|(_, to, _)| *to == self.device_id) {
            #[allow(clippy::unwrap_used)] // Safe: position() just confirmed pos exists
            let (from, _, message) = queue.remove(pos).unwrap();
            Ok((from, message))
        } else {
            Err(NetworkError::ReceiveFailed(
                "Timeout waiting for message".to_string(),
            ))
        }
    }

    async fn receive_from(&self, peer_id: Uuid) -> Result<Vec<u8>, NetworkError> {
        let mut queue = self.global_message_queue.lock().await;

        // Find message from specific peer to this device
        if let Some(pos) = queue
            .iter()
            .position(|(from, to, _)| *from == peer_id && *to == self.device_id)
        {
            #[allow(clippy::unwrap_used)] // Safe: position() just confirmed pos exists
            let (_, _, message) = queue.remove(pos).unwrap();
            Ok(message)
        } else {
            Err(NetworkError::ReceiveFailed(
                "Timeout waiting for message".to_string(),
            ))
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

        Ok(Box::pin(
            tokio_stream::wrappers::UnboundedReceiverStream::new(receiver),
        ))
    }
}
