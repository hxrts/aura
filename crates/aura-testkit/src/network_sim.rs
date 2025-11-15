//! Network simulation utilities for testing distributed protocols
//!
//! This module provides tools for simulating network conditions,
//! including latency, packet loss, partitions, and bandwidth limits.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use aura_core::{AuraError, AuraResult, DeviceId};
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use tokio::sync::{mpsc, Mutex};
use tokio::time::sleep;

/// Network simulator for testing distributed scenarios
pub struct NetworkSimulator {
    /// Network conditions between peers
    conditions: Arc<Mutex<HashMap<(DeviceId, DeviceId), NetworkCondition>>>,
    /// Default conditions for new connections
    default_conditions: NetworkCondition,
    /// Random number generator for deterministic simulation
    rng: Arc<Mutex<StdRng>>,
}

/// Network conditions between two peers
#[derive(Clone, Debug)]
pub struct NetworkCondition {
    /// Base latency for messages
    pub latency: Duration,
    /// Latency jitter (±)
    pub jitter: Duration,
    /// Packet loss probability (0.0 - 1.0)
    pub loss_rate: f64,
    /// Bandwidth limit in bytes per second
    pub bandwidth: Option<u64>,
    /// Whether the connection is partitioned
    pub partitioned: bool,
}

impl Default for NetworkCondition {
    fn default() -> Self {
        Self {
            latency: Duration::from_millis(10),
            jitter: Duration::from_millis(2),
            loss_rate: 0.0,
            bandwidth: None,
            partitioned: false,
        }
    }
}

impl NetworkCondition {
    /// Create perfect network conditions (for testing)
    pub fn perfect() -> Self {
        Self {
            latency: Duration::ZERO,
            jitter: Duration::ZERO,
            loss_rate: 0.0,
            bandwidth: None,
            partitioned: false,
        }
    }

    /// Create realistic WAN conditions
    pub fn wan() -> Self {
        Self {
            latency: Duration::from_millis(50),
            jitter: Duration::from_millis(10),
            loss_rate: 0.01,                   // 1% loss
            bandwidth: Some(10 * 1024 * 1024), // 10 MB/s
            partitioned: false,
        }
    }

    /// Create poor network conditions
    pub fn poor() -> Self {
        Self {
            latency: Duration::from_millis(200),
            jitter: Duration::from_millis(50),
            loss_rate: 0.05,                  // 5% loss
            bandwidth: Some(1 * 1024 * 1024), // 1 MB/s
            partitioned: false,
        }
    }
}

impl NetworkSimulator {
    /// Create a new network simulator with default conditions
    pub fn new() -> Self {
        Self::with_seed(42) // Deterministic by default
    }

    /// Create a new network simulator with a specific seed
    pub fn with_seed(seed: u64) -> Self {
        Self {
            conditions: Arc::new(Mutex::new(HashMap::new())),
            default_conditions: NetworkCondition::default(),
            rng: Arc::new(Mutex::new(StdRng::seed_from_u64(seed))),
        }
    }

    /// Set network conditions between two peers
    pub async fn set_conditions(&self, from: DeviceId, to: DeviceId, conditions: NetworkCondition) {
        let mut map = self.conditions.lock().await;
        map.insert((from, to), conditions);
    }

    /// Create a network partition between groups of peers
    pub async fn partition(&self, group1: Vec<DeviceId>, group2: Vec<DeviceId>) {
        let mut map = self.conditions.lock().await;

        for device1 in &group1 {
            for device2 in &group2 {
                // Partition in both directions
                let mut condition = self.default_conditions.clone();
                condition.partitioned = true;

                map.insert((*device1, *device2), condition.clone());
                map.insert((*device2, *device1), condition);
            }
        }
    }

    /// Heal a network partition
    pub async fn heal_partition(&self) {
        let mut map = self.conditions.lock().await;

        // Remove all partitioned conditions
        map.retain(|_, condition| !condition.partitioned);
    }

    /// Simulate sending a message through the network
    pub async fn simulate_send(
        &self,
        from: DeviceId,
        to: DeviceId,
        message_size: usize,
    ) -> AuraResult<()> {
        let conditions = {
            let map = self.conditions.lock().await;
            map.get(&(from, to))
                .cloned()
                .unwrap_or_else(|| self.default_conditions.clone())
        };

        // Check if partitioned
        if conditions.partitioned {
            return Err(AuraError::invalid("Network partition"));
        }

        // Simulate packet loss
        let should_drop = {
            let mut rng = self.rng.lock().await;
            rng.gen::<f64>() < conditions.loss_rate
        };

        if should_drop {
            return Err(AuraError::invalid("Packet lost"));
        }

        // Calculate latency with jitter
        let latency = {
            let mut rng = self.rng.lock().await;
            let jitter_ms = if conditions.jitter > Duration::ZERO {
                let max_jitter = conditions.jitter.as_millis() as i64;
                rng.gen_range(-max_jitter..=max_jitter)
            } else {
                0
            };

            let base_ms = conditions.latency.as_millis() as i64;
            let total_ms = (base_ms + jitter_ms).max(0) as u64;
            Duration::from_millis(total_ms)
        };

        // Simulate bandwidth delay
        if let Some(bandwidth) = conditions.bandwidth {
            let transmission_time = Duration::from_secs_f64(message_size as f64 / bandwidth as f64);
            sleep(transmission_time).await;
        }

        // Simulate network latency
        sleep(latency).await;

        Ok(())
    }
}

/// Network topology builder for complex test scenarios
pub struct NetworkTopology {
    simulator: NetworkSimulator,
    devices: Vec<DeviceId>,
}

impl NetworkTopology {
    /// Create a new topology with the given devices
    pub fn new(devices: Vec<DeviceId>) -> Self {
        Self {
            simulator: NetworkSimulator::new(),
            devices,
        }
    }

    /// Create a star topology with one central node
    pub async fn star(mut self, center: DeviceId) -> NetworkSimulator {
        for device in &self.devices {
            if *device != center {
                // Good conditions to center
                self.simulator
                    .set_conditions(*device, center, NetworkCondition::default())
                    .await;

                self.simulator
                    .set_conditions(center, *device, NetworkCondition::default())
                    .await;

                // Poor conditions between edge nodes
                for other in &self.devices {
                    if *other != center && *other != *device {
                        self.simulator
                            .set_conditions(*device, *other, NetworkCondition::poor())
                            .await;
                    }
                }
            }
        }

        self.simulator
    }

    /// Create a ring topology
    pub async fn ring(mut self) -> NetworkSimulator {
        let n = self.devices.len();

        for i in 0..n {
            let next = (i + 1) % n;

            // Good conditions to neighbors
            self.simulator
                .set_conditions(
                    self.devices[i],
                    self.devices[next],
                    NetworkCondition::default(),
                )
                .await;

            self.simulator
                .set_conditions(
                    self.devices[next],
                    self.devices[i],
                    NetworkCondition::default(),
                )
                .await;
        }

        self.simulator
    }

    /// Create a fully connected mesh
    pub async fn mesh(mut self) -> NetworkSimulator {
        for i in 0..self.devices.len() {
            for j in i + 1..self.devices.len() {
                self.simulator
                    .set_conditions(
                        self.devices[i],
                        self.devices[j],
                        NetworkCondition::default(),
                    )
                    .await;

                self.simulator
                    .set_conditions(
                        self.devices[j],
                        self.devices[i],
                        NetworkCondition::default(),
                    )
                    .await;
            }
        }

        self.simulator
    }
}

/// Message delivery tracker for assertions
pub struct DeliveryTracker {
    sent: Arc<Mutex<Vec<(DeviceId, DeviceId, String)>>>,
    received: Arc<Mutex<Vec<(DeviceId, DeviceId, String)>>>,
}

impl DeliveryTracker {
    pub fn new() -> Self {
        Self {
            sent: Arc::new(Mutex::new(Vec::new())),
            received: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub async fn record_sent(&self, from: DeviceId, to: DeviceId, msg_id: String) {
        self.sent.lock().await.push((from, to, msg_id));
    }

    pub async fn record_received(&self, from: DeviceId, to: DeviceId, msg_id: String) {
        self.received.lock().await.push((from, to, msg_id));
    }

    pub async fn assert_all_delivered(&self) -> AuraResult<()> {
        let sent = self.sent.lock().await;
        let received = self.received.lock().await;

        for sent_msg in sent.iter() {
            if !received.contains(sent_msg) {
                return Err(AuraError::invalid(format!(
                    "Message {:?} was sent but not received",
                    sent_msg
                )));
            }
        }

        Ok(())
    }
}
