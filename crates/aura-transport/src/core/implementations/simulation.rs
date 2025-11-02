//! Simulation transport implementation for deterministic testing

use crate::{
    core::traits::Transport, error::TransportErrorBuilder, TransportEnvelope, TransportResult,
};
use async_trait::async_trait;
use aura_types::DeviceId;
use std::{collections::HashMap, sync::Arc, time::Duration};
use tokio::sync::{mpsc, Mutex, RwLock};

/// Simulation transport for deterministic testing with controllable network conditions
#[derive(Clone)]
pub struct SimulationTransport {
    device_id: DeviceId,
    /// Global simulation state shared across all devices
    simulation_state: Arc<RwLock<SimulationState>>,
    /// Local message receiver
    receiver: Arc<Mutex<mpsc::UnboundedReceiver<TransportEnvelope>>>,
    /// Local message sender
    sender: mpsc::UnboundedSender<TransportEnvelope>,
    /// Network conditions configuration
    network_config: Arc<RwLock<NetworkConfig>>,
}

/// Global simulation state for coordinating all transport instances
#[derive(Default)]
struct SimulationState {
    /// Active transport instances by device ID
    transports: HashMap<DeviceId, mpsc::UnboundedSender<TransportEnvelope>>,
    /// Message delivery delays (in milliseconds)
    message_delays: HashMap<(DeviceId, DeviceId), u64>,
    /// Connection status between devices
    connections: HashMap<(DeviceId, DeviceId), bool>,
    /// Partition status - devices in different partitions cannot communicate
    partitions: HashMap<DeviceId, u32>,
}

/// Network configuration for simulation
#[derive(Clone)]
pub struct NetworkConfig {
    /// Base message delivery delay in milliseconds
    pub base_delay_ms: u64,
    /// Random jitter range in milliseconds
    pub jitter_ms: u64,
    /// Packet loss probability (0.0 to 1.0)
    pub packet_loss_rate: f64,
    /// Whether Byzantine behavior is enabled
    pub byzantine_enabled: bool,
    /// Partition ID for this device (devices in different partitions cannot communicate)
    pub partition_id: u32,
}

impl Default for NetworkConfig {
    fn default() -> Self {
        Self {
            base_delay_ms: 10,
            jitter_ms: 5,
            packet_loss_rate: 0.0,
            byzantine_enabled: false,
            partition_id: 0,
        }
    }
}

impl SimulationTransport {
    /// Create a new simulation transport
    pub fn new(device_id: DeviceId) -> Self {
        let (sender, receiver) = mpsc::unbounded_channel();
        Self {
            device_id,
            simulation_state: Arc::new(RwLock::new(SimulationState::default())),
            receiver: Arc::new(Mutex::new(receiver)),
            sender,
            network_config: Arc::new(RwLock::new(NetworkConfig::default())),
        }
    }

    /// Create a new simulation transport with shared simulation state
    fn with_shared_state(
        device_id: DeviceId,
        simulation_state: Arc<RwLock<SimulationState>>,
    ) -> Self {
        let (sender, receiver) = mpsc::unbounded_channel();
        Self {
            device_id,
            simulation_state,
            receiver: Arc::new(Mutex::new(receiver)),
            sender,
            network_config: Arc::new(RwLock::new(NetworkConfig::default())),
        }
    }

    /// Update network configuration for this transport
    pub async fn set_network_config(&self, config: NetworkConfig) {
        *self.network_config.write().await = config;
    }

    /// Simulate network partition by setting partition ID
    pub async fn set_partition(&self, partition_id: u32) {
        self.network_config.write().await.partition_id = partition_id;
        self.simulation_state
            .write()
            .await
            .partitions
            .insert(self.device_id, partition_id);
    }

    /// Set message delay between two devices
    pub async fn set_message_delay(&self, from: DeviceId, to: DeviceId, delay_ms: u64) {
        self.simulation_state
            .write()
            .await
            .message_delays
            .insert((from, to), delay_ms);
    }

    /// Enable Byzantine behavior for this transport
    pub async fn enable_byzantine_behavior(&self) {
        self.network_config.write().await.byzantine_enabled = true;
    }

    /// Check if message should be delivered based on network conditions
    async fn should_deliver_message(&self, to: DeviceId) -> bool {
        let config = self.network_config.read().await;
        let state = self.simulation_state.read().await;

        // Check partition isolation
        let my_partition = state.partitions.get(&self.device_id).unwrap_or(&0);
        let target_partition = state.partitions.get(&to).unwrap_or(&0);
        if my_partition != target_partition {
            return false;
        }

        // Check packet loss
        if config.packet_loss_rate > 0.0 {
            let random_value: f64 = fastrand::f64();
            if random_value < config.packet_loss_rate {
                return false;
            }
        }

        true
    }

    /// Calculate message delivery delay
    async fn calculate_delay(&self, to: DeviceId) -> Duration {
        let config = self.network_config.read().await;
        let state = self.simulation_state.read().await;

        // Check for specific delay configuration
        let base_delay = state
            .message_delays
            .get(&(self.device_id, to))
            .unwrap_or(&config.base_delay_ms);

        // Add jitter
        let jitter = if config.jitter_ms > 0 {
            fastrand::u64(0..=config.jitter_ms)
        } else {
            0
        };

        Duration::from_millis(base_delay + jitter)
    }
}

#[async_trait]
impl Transport for SimulationTransport {
    async fn send(&self, envelope: TransportEnvelope) -> TransportResult<()> {
        // Check if message should be delivered
        if !self.should_deliver_message(envelope.to).await {
            tracing::debug!(
                "Simulation: Message from {} to {} dropped due to network conditions",
                self.device_id,
                envelope.to
            );
            return Ok(()); // Simulate dropped packet
        }

        // Calculate delivery delay
        let delay = self.calculate_delay(envelope.to).await;
        let target_device_id = envelope.to; // Capture before move

        let state = self.simulation_state.read().await;
        if let Some(target_sender) = state.transports.get(&envelope.to) {
            let target_sender = target_sender.clone();
            drop(state);

            // Simulate network delay
            if delay > Duration::from_millis(0) {
                let envelope_clone = envelope.clone();
                tokio::spawn(async move {
                    tokio::time::sleep(delay).await;
                    if target_sender.send(envelope_clone).is_err() {
                        tracing::warn!("Failed to deliver delayed message");
                    }
                });
            } else {
                target_sender
                    .send(envelope)
                    .map_err(|_| TransportErrorBuilder::connection("Target device disconnected"))?;
            }

            tracing::debug!(
                "Simulation: Message sent from {} to {} with {}ms delay",
                self.device_id,
                target_device_id,
                delay.as_millis()
            );
            Ok(())
        } else {
            Err(TransportErrorBuilder::peer_unreachable(
                envelope.to.to_string(),
            ))
        }
    }

    async fn receive(&self, timeout: Duration) -> TransportResult<Option<TransportEnvelope>> {
        let mut receiver = self.receiver.lock().await;

        match tokio::time::timeout(timeout, receiver.recv()).await {
            Ok(Some(envelope)) => {
                tracing::debug!(
                    "Simulation: Message received by {} from {}",
                    self.device_id,
                    envelope.from
                );
                Ok(Some(envelope))
            }
            Ok(None) => Ok(None), // Channel closed
            Err(_) => Ok(None),   // Timeout
        }
    }

    async fn connect(&self, peer_id: DeviceId) -> TransportResult<()> {
        let mut state = self.simulation_state.write().await;
        state.connections.insert((self.device_id, peer_id), true);
        state.connections.insert((peer_id, self.device_id), true);

        tracing::debug!(
            "Simulation: Connection established between {} and {}",
            self.device_id,
            peer_id
        );
        Ok(())
    }

    async fn disconnect(&self, peer_id: DeviceId) -> TransportResult<()> {
        let mut state = self.simulation_state.write().await;
        state.connections.remove(&(self.device_id, peer_id));
        state.connections.remove(&(peer_id, self.device_id));

        tracing::debug!(
            "Simulation: Connection disconnected between {} and {}",
            self.device_id,
            peer_id
        );
        Ok(())
    }

    async fn is_reachable(&self, peer_id: DeviceId) -> bool {
        let state = self.simulation_state.read().await;

        // Check partition isolation
        let my_partition = state.partitions.get(&self.device_id).unwrap_or(&0);
        let target_partition = state.partitions.get(&peer_id).unwrap_or(&0);
        if my_partition != target_partition {
            return false;
        }

        // Check if peer exists in simulation
        state.transports.contains_key(&peer_id)
    }

    async fn start(&mut self) -> TransportResult<()> {
        // Register this transport in the global simulation state
        self.simulation_state
            .write()
            .await
            .transports
            .insert(self.device_id, self.sender.clone());

        tracing::info!("Simulation transport started for device {}", self.device_id);
        Ok(())
    }

    async fn stop(&mut self) -> TransportResult<()> {
        // Unregister this transport from the global simulation state
        self.simulation_state
            .write()
            .await
            .transports
            .remove(&self.device_id);

        tracing::info!("Simulation transport stopped for device {}", self.device_id);
        Ok(())
    }

    fn transport_type(&self) -> &'static str {
        "simulation"
    }
}

/// Helper functions for creating simulation environments
impl SimulationTransport {
    /// Create a connected network of simulation transports
    pub async fn create_network(
        device_ids: Vec<DeviceId>,
    ) -> TransportResult<Vec<SimulationTransport>> {
        let shared_state = Arc::new(RwLock::new(SimulationState::default()));
        let mut transports = Vec::new();

        for device_id in device_ids {
            let transport = SimulationTransport::with_shared_state(device_id, shared_state.clone());
            transports.push(transport);
        }

        // Start all transports to register them
        for transport in &mut transports {
            transport.start().await?;
        }

        // Connect all devices to each other
        for i in 0..transports.len() {
            for j in 0..transports.len() {
                if i != j {
                    let peer_id = transports[j].device_id;
                    transports[i].connect(peer_id).await?;
                }
            }
        }

        Ok(transports)
    }

    /// Create a partitioned network for testing network splits
    pub async fn create_partitioned_network(
        partition_a: Vec<DeviceId>,
        partition_b: Vec<DeviceId>,
    ) -> TransportResult<(Vec<SimulationTransport>, Vec<SimulationTransport>)> {
        let shared_state = Arc::new(RwLock::new(SimulationState::default()));

        let mut transports_a = Vec::new();
        let mut transports_b = Vec::new();

        // Create transports for partition A
        for device_id in partition_a {
            let mut transport =
                SimulationTransport::with_shared_state(device_id, shared_state.clone());
            transport.start().await?;
            transport.set_partition(0).await;
            transports_a.push(transport);
        }

        // Create transports for partition B
        for device_id in partition_b {
            let mut transport =
                SimulationTransport::with_shared_state(device_id, shared_state.clone());
            transport.start().await?;
            transport.set_partition(1).await;
            transports_b.push(transport);
        }

        // Connect devices within each partition
        for i in 0..transports_a.len() {
            for j in 0..transports_a.len() {
                if i != j {
                    let peer_id = transports_a[j].device_id;
                    transports_a[i].connect(peer_id).await?;
                }
            }
        }

        for i in 0..transports_b.len() {
            for j in 0..transports_b.len() {
                if i != j {
                    let peer_id = transports_b[j].device_id;
                    transports_b[i].connect(peer_id).await?;
                }
            }
        }

        Ok((transports_a, transports_b))
    }
}
