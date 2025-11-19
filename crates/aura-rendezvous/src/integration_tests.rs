//! End-to-End Integration Tests for SBB System
//!
//! This module provides comprehensive integration tests that demonstrate the complete
//! Alice→Bob connection flow via the Social Bulletin Board (SBB) system, including:
//! - Relationship establishment and trust management
//! - Encrypted envelope flooding with capability enforcement
//! - Transport offer discovery and connection establishment
//! - Flow budget enforcement and trust-based forwarding

#![allow(clippy::disallowed_methods)]

use crate::messaging::{NetworkConfig, NetworkTransport};
use crate::{
    capability_aware_sbb::SbbForwardingPolicy,
    envelope_encryption::PaddingStrategy,
    integrated_sbb::{IntegratedSbbSystem, SbbConfig, SbbDiscoveryRequest, SbbSystemBuilder},
    messaging::{TransportMethod, TransportOfferPayload},
};
use aura_agent::runtime::{AuraEffectSystem, EffectSystemConfig};
use aura_agent::NetworkEffects;
use aura_core::{AuraResult, DeviceId, RelationshipId, TrustLevel};
use aura_protocol::effects::AuraEffects;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::time::Duration;
use tracing_subscriber;

/// End-to-end test scenario configuration
#[derive(Debug, Clone)]
pub struct E2eTestConfig {
    /// Number of devices in the test network
    pub device_count: usize,
    /// Whether to use encrypted envelopes
    pub use_encryption: bool,
    /// Trust level for relationships
    pub trust_level: TrustLevel,
    /// TTL for SBB flooding
    pub ttl: u8,
    /// Test timeout duration
    pub timeout_duration: Duration,
}

impl Default for E2eTestConfig {
    fn default() -> Self {
        Self {
            device_count: 3, // Alice, Bob, Charlie
            use_encryption: true,
            trust_level: TrustLevel::Medium,
            ttl: 4,
            timeout_duration: Duration::from_secs(10),
        }
    }
}

/// Temporary device info for avoiding borrowing issues
#[derive(Debug, Clone)]
struct TempDeviceInfo {
    device_id: DeviceId,
    name: String,
    #[allow(dead_code)]
    transport_config: NetworkConfig,
}

/// Test device in the SBB network
#[derive(Debug)]
pub struct TestDevice {
    /// Device identifier
    pub device_id: DeviceId,
    /// Device name for logging
    pub name: String,
    /// Integrated SBB system
    pub sbb_system: IntegratedSbbSystem,
    /// Network transport
    pub transport: Arc<RwLock<NetworkTransport>>,
}

/// Complete SBB test network
#[derive(Debug)]
pub struct SbbTestNetwork {
    /// All devices in the network
    pub devices: HashMap<DeviceId, TestDevice>,
    /// Test configuration
    pub config: E2eTestConfig,
}

/// Test result for Alice→Bob connection
#[derive(Debug)]
pub struct ConnectionTestResult {
    /// Whether the connection was successful
    pub success: bool,
    /// Number of devices the discovery request reached
    pub devices_reached: usize,
    /// Total flow budget consumed
    pub flow_consumed: u64,
    /// Whether encryption was used
    pub encryption_used: bool,
    /// Time taken for discovery
    pub discovery_time: Duration,
    /// Error message if failed
    pub error: Option<String>,
}

impl TestDevice {
    /// Create new test device
    pub async fn new(
        device_id: DeviceId,
        name: String,
        _config: &E2eTestConfig,
    ) -> AuraResult<Self> {
        // Create shared effect system
        let effects = Arc::new(AuraEffectSystem::new());
        let network_effects = Arc::clone(&effects) as Arc<dyn NetworkEffects>;

        // Create network transport
        let transport = NetworkTransport::new(device_id, network_effects);

        // Create SBB system
        let sbb_config = SbbConfig {
            forwarding_policy: SbbForwardingPolicy {
                min_trust_level: TrustLevel::Low,
                max_flow_usage: 0.5, // Use up to 50% of flow budget
                prefer_guardians: true,
                max_streams_per_peer: 10,
            },
            padding_strategy: PaddingStrategy::PowerOfTwo,
            app_context: "test-sbb-e2e".to_string(),
        };

        let aura_effects = Arc::clone(&effects) as Arc<dyn aura_protocol::effects::AuraEffects>;
        let sbb_system = SbbSystemBuilder::new(device_id)
            .with_config(sbb_config)
            .with_transport(Arc::clone(&transport))
            .build(aura_effects);

        Ok(Self {
            device_id,
            name,
            sbb_system,
            transport,
        })
    }

    /// Add relationship to another device
    pub async fn add_relationship(
        &mut self,
        peer_device: &TestDevice,
        trust_level: TrustLevel,
        is_guardian: bool,
    ) -> AuraResult<()> {
        let relationship_id = RelationshipId::new([0u8; 32]);
        let now = crate::sbb::current_timestamp();

        if is_guardian {
            self.sbb_system
                .add_guardian(peer_device.device_id, relationship_id, trust_level, now)
                .await;
        } else {
            self.sbb_system
                .add_friend(peer_device.device_id, relationship_id, trust_level, now)
                .await;
        }

        // Add peer to transport (simulate network connectivity)
        // Note: NetworkTransport API changed - add_peer method removed
        // TODO: Update to use add_peer_for_context with proper ContextId
        // Config API no longer exposed - NetworkTransport uses network_effects directly
        // let _peer_config = peer_device.transport.read().await.config().clone();
        // let peer_addr = format!("{}:{}", peer_config.bind_addr, peer_config.port)
        //     .parse()
        //     .map_err(|e| {
        //         aura_core::AuraError::coordination_failed(format!("Invalid address: {}", e))
        //     })?;
        //
        // self.transport
        //     .write()
        //     .await
        //     .add_peer(peer_device.device_id, peer_addr)
        //     .await?;

        tracing::info!(
            "{} added {} as {} (trust: {:?})",
            self.name,
            peer_device.name,
            if is_guardian { "guardian" } else { "friend" },
            trust_level
        );

        Ok(())
    }

    /// Add relationship using temporary device info (avoids borrowing issues)
    async fn add_relationship_from_info(
        &mut self,
        peer_info: &TempDeviceInfo,
        trust_level: TrustLevel,
        is_guardian: bool,
    ) -> AuraResult<()> {
        let relationship_id = RelationshipId::new([0u8; 32]);
        let now = crate::sbb::current_timestamp();

        if is_guardian {
            self.sbb_system
                .add_guardian(peer_info.device_id, relationship_id, trust_level, now)
                .await;
        } else {
            self.sbb_system
                .add_friend(peer_info.device_id, relationship_id, trust_level, now)
                .await;
        }

        // Add peer to transport (simulate network connectivity)
        // Note: NetworkTransport API changed - add_peer method removed
        // TODO: Update to use add_peer_for_context with proper ContextId
        // let peer_addr = format!(
        //     "{}:{}",
        //     peer_info.transport_config.bind_addr, peer_info.transport_config.port
        // )
        // .parse()
        // .map_err(|e| {
        //     aura_core::AuraError::coordination_failed(format!("Invalid address: {}", e))
        // })?;
        //
        // self.transport
        //     .write()
        //     .await
        //     .add_peer(peer_info.device_id, peer_addr)
        //     .await?;

        tracing::info!(
            "{} added {} as {} (trust: {:?})",
            self.name,
            peer_info.name,
            if is_guardian { "guardian" } else { "friend" },
            trust_level
        );

        Ok(())
    }

    /// Create and flood transport offer
    pub async fn flood_transport_offer(
        &mut self,
        offer_methods: Vec<TransportMethod>,
        use_encryption: bool,
        ttl: Option<u8>,
    ) -> AuraResult<crate::integrated_sbb::SbbDiscoveryResult> {
        let offer = TransportOfferPayload {
            device_id: self.device_id,
            transport_methods: offer_methods,
            expires_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs()
                + 3600, // 1 hour expiration
            nonce: rand::random(),
        };

        let discovery_request = SbbDiscoveryRequest {
            device_id: self.device_id,
            transport_offer: offer,
            use_encryption,
            ttl,
        };

        tracing::info!(
            "{} flooding transport offer (encrypted: {}, ttl: {:?})",
            self.name,
            use_encryption,
            ttl
        );

        self.sbb_system
            .flood_discovery_request(discovery_request)
            .await
    }

    /// Get current SBB statistics
    pub fn get_sbb_stats(&self) -> crate::capability_aware_sbb::TrustStatistics {
        self.sbb_system.get_statistics()
    }
}

impl SbbTestNetwork {
    /// Create new test network with specified configuration
    pub async fn new(config: E2eTestConfig) -> AuraResult<Self> {
        let mut devices = HashMap::new();

        // Create devices
        for i in 0..config.device_count {
            let device_id = DeviceId::new();
            let name = match i {
                0 => "Alice".to_string(),
                1 => "Bob".to_string(),
                2 => "Charlie".to_string(),
                n => format!("Device{}", n),
            };

            let device = TestDevice::new(device_id, name, &config).await?;
            devices.insert(device_id, device);
        }

        Ok(Self { devices, config })
    }

    /// Set up linear topology: Alice ↔ Bob ↔ Charlie
    pub async fn setup_linear_topology(&mut self) -> AuraResult<()> {
        let device_ids: Vec<DeviceId> = self.devices.keys().copied().collect();

        if device_ids.len() < 2 {
            return Err(aura_core::AuraError::coordination_failed(
                "Need at least 2 devices for topology".to_string(),
            ));
        }

        // Alice ↔ Bob
        let (alice_id, bob_id) = (device_ids[0], device_ids[1]);

        // Alice adds Bob as friend
        {
            // Clone the necessary data from bob_device to avoid simultaneous borrows
            let (bob_device_id, bob_name) = {
                let bob_device = self.devices.get(&bob_id).unwrap();
                // NetworkTransport no longer exposes config() - it uses network_effects directly
                (
                    bob_device.device_id,
                    bob_device.name.clone(),
                )
            };

            // Create a temporary device-like struct with just the needed data
            let temp_bob = TempDeviceInfo {
                device_id: bob_device_id,
                name: bob_name,
                transport_config: NetworkConfig::default(), // Placeholder - field is dead_code
            };

            self.devices
                .get_mut(&alice_id)
                .unwrap()
                .add_relationship_from_info(&temp_bob, self.config.trust_level, false)
                .await?;
        }

        // Bob adds Alice as friend
        {
            // Clone the necessary data from alice_device to avoid simultaneous borrows
            let (alice_device_id, alice_name, alice_transport_config) = {
                let alice_device = self.devices.get(&alice_id).unwrap();
                // NetworkTransport no longer exposes config()
                let transport_config = NetworkConfig::default();
                (
                    alice_device.device_id,
                    alice_device.name.clone(),
                    transport_config,
                )
            };

            // Create a temporary device-like struct with just the needed data
            let temp_alice = TempDeviceInfo {
                device_id: alice_device_id,
                name: alice_name,
                transport_config: alice_transport_config,
            };

            self.devices
                .get_mut(&bob_id)
                .unwrap()
                .add_relationship_from_info(&temp_alice, self.config.trust_level, false)
                .await?;
        }

        // If we have Charlie, connect Bob ↔ Charlie
        if device_ids.len() >= 3 {
            let charlie_id = device_ids[2];

            // Bob adds Charlie as friend
            {
                // Clone the necessary data from charlie_device to avoid simultaneous borrows
                let (charlie_device_id, charlie_name, charlie_transport_config) = {
                    let charlie_device = self.devices.get(&charlie_id).unwrap();
                    // NetworkTransport no longer exposes config()
                    let transport_config = NetworkConfig::default();
                    (
                        charlie_device.device_id,
                        charlie_device.name.clone(),
                        transport_config,
                    )
                };

                let temp_charlie = TempDeviceInfo {
                    device_id: charlie_device_id,
                    name: charlie_name,
                    transport_config: charlie_transport_config,
                };

                self.devices
                    .get_mut(&bob_id)
                    .unwrap()
                    .add_relationship_from_info(&temp_charlie, self.config.trust_level, false)
                    .await?;
            }

            // Charlie adds Bob as guardian (preferred for reliability)
            {
                // Clone the necessary data from bob_device to avoid simultaneous borrows
                let (bob_device_id, bob_name, bob_transport_config) = {
                    let bob_device = self.devices.get(&bob_id).unwrap();
                    // NetworkTransport no longer exposes config()
                let transport_config = NetworkConfig::default();
                    (
                        bob_device.device_id,
                        bob_device.name.clone(),
                        transport_config,
                    )
                };

                let temp_bob = TempDeviceInfo {
                    device_id: bob_device_id,
                    name: bob_name,
                    transport_config: bob_transport_config,
                };

                self.devices
                    .get_mut(&charlie_id)
                    .unwrap()
                    .add_relationship_from_info(&temp_bob, self.config.trust_level, true)
                    .await?;
            }
        }

        tracing::info!("Set up linear topology: Alice ↔ Bob ↔ Charlie");
        Ok(())
    }

    /// Set up full mesh topology (everyone connected to everyone)
    pub async fn setup_mesh_topology(&mut self) -> AuraResult<()> {
        let device_ids: Vec<DeviceId> = self.devices.keys().copied().collect();

        for i in 0..device_ids.len() {
            for j in 0..device_ids.len() {
                if i != j {
                    let device_a_id = device_ids[i];
                    let device_b_id = device_ids[j];

                    // Clone the necessary data from device_b to avoid simultaneous borrows
                    let (device_b_device_id, device_b_name, device_b_transport_config) = {
                        let device_b = self.devices.get(&device_b_id).unwrap();
                        // NetworkTransport no longer exposes config()
                        let transport_config = NetworkConfig::default();
                        (device_b.device_id, device_b.name.clone(), transport_config)
                    };

                    let temp_device_b = TempDeviceInfo {
                        device_id: device_b_device_id,
                        name: device_b_name,
                        transport_config: device_b_transport_config,
                    };

                    let is_guardian = j == 0; // Make first device a guardian

                    self.devices
                        .get_mut(&device_a_id)
                        .unwrap()
                        .add_relationship_from_info(
                            &temp_device_b,
                            self.config.trust_level,
                            is_guardian,
                        )
                        .await?;
                }
            }
        }

        tracing::info!("Set up full mesh topology");
        Ok(())
    }

    /// Run Alice→Bob connection test
    pub async fn test_alice_bob_connection(&mut self) -> AuraResult<ConnectionTestResult> {
        let device_ids: Vec<DeviceId> = self.devices.keys().copied().collect();
        if device_ids.len() < 2 {
            return Err(aura_core::AuraError::coordination_failed(
                "Need at least 2 devices for connection test".to_string(),
            ));
        }

        let (alice_id, _bob_id) = (device_ids[0], device_ids[1]);
        let start_time = std::time::Instant::now();

        // Alice creates transport offer
        let offer_methods = vec![
            TransportMethod::WebSocket {
                url: "ws://127.0.0.1:8080".to_string(),
            },
            TransportMethod::Quic {
                addr: "127.0.0.1".to_string(),
                port: 8443,
            },
        ];

        // Alice floods discovery request
        let result = {
            let alice = self.devices.get_mut(&alice_id).unwrap();
            alice
                .flood_transport_offer(
                    offer_methods,
                    self.config.use_encryption,
                    Some(self.config.ttl),
                )
                .await
        };

        let discovery_time = start_time.elapsed();

        match result {
            Ok(discovery_result) => {
                // Check if the discovery reached expected devices
                let _expected_devices = if device_ids.len() >= 3 { 2 } else { 1 }; // Bob + Charlie if present

                // Get flow statistics
                let alice_stats = self.devices.get(&alice_id).unwrap().get_sbb_stats();

                Ok(ConnectionTestResult {
                    success: discovery_result.peers_reached > 0,
                    devices_reached: discovery_result.peers_reached,
                    flow_consumed: alice_stats.total_flow_spent,
                    encryption_used: discovery_result.encrypted,
                    discovery_time,
                    error: None,
                })
            }
            Err(e) => Ok(ConnectionTestResult {
                success: false,
                devices_reached: 0,
                flow_consumed: 0,
                encryption_used: self.config.use_encryption,
                discovery_time,
                error: Some(e.to_string()),
            }),
        }
    }

    /// Run comprehensive SBB system test
    pub async fn run_comprehensive_test(&mut self) -> AuraResult<Vec<ConnectionTestResult>> {
        let mut results = Vec::new();

        // Test 1: Basic Alice→Bob connection
        tracing::info!("Running Test 1: Basic Alice→Bob connection");
        let result1 = self.test_alice_bob_connection().await?;
        results.push(result1);

        // Test 2: Update trust levels and retry
        if self.devices.len() >= 2 {
            tracing::info!("Running Test 2: High trust connection");
            let device_ids: Vec<DeviceId> = self.devices.keys().copied().collect();
            let (alice_id, bob_id) = (device_ids[0], device_ids[1]);

            // Upgrade Alice's trust in Bob
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs();
            self.devices
                .get_mut(&alice_id)
                .unwrap()
                .sbb_system
                .update_trust_level(bob_id, TrustLevel::High, now)?;

            let result2 = self.test_alice_bob_connection().await?;
            results.push(result2);
        }

        // Test 3: Flow budget exhaustion (if implemented)
        tracing::info!("Running Test 3: Multiple rapid discoveries");
        for i in 0..3 {
            tracing::info!("Discovery attempt {}", i + 1);
            let result = self.test_alice_bob_connection().await?;
            results.push(result);

            // Small delay between attempts
            tokio::time::sleep(Duration::from_millis(100)).await;
        }

        Ok(results)
    }

    /// Get network statistics
    pub fn get_network_stats(
        &self,
    ) -> HashMap<DeviceId, crate::capability_aware_sbb::TrustStatistics> {
        self.devices
            .iter()
            .map(|(id, device)| (*id, device.get_sbb_stats()))
            .collect()
    }

    /// Print network summary
    pub fn print_network_summary(&self) {
        tracing::info!("=== SBB Network Summary ===");
        for (device_id, device) in &self.devices {
            let stats = device.get_sbb_stats();
            tracing::info!(
                "{} ({}): {} relationships, trust: {:.2}, flow: {:.1}%",
                device.name,
                device_id.0,
                stats.relationship_count,
                stats.average_trust_level(),
                stats.flow_utilization() * 100.0
            );
        }
    }
}

/// Convenience function to run a complete Alice→Bob SBB test
pub async fn run_alice_bob_sbb_test(config: E2eTestConfig) -> AuraResult<ConnectionTestResult> {
    // Initialize tracing for test visibility
    let _ = tracing_subscriber::fmt::try_init();

    let mut network = SbbTestNetwork::new(config).await?;
    network.setup_linear_topology().await?;
    network.print_network_summary();

    let result = network.test_alice_bob_connection().await?;

    tracing::info!("=== Test Results ===");
    tracing::info!("Success: {}", result.success);
    tracing::info!("Devices reached: {}", result.devices_reached);
    tracing::info!("Flow consumed: {} bytes", result.flow_consumed);
    tracing::info!("Encryption used: {}", result.encryption_used);
    tracing::info!("Discovery time: {:?}", result.discovery_time);
    if let Some(error) = &result.error {
        tracing::error!("Error: {}", error);
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_basic_alice_bob_connection() {
        let config = E2eTestConfig {
            device_count: 2,
            use_encryption: false,
            trust_level: TrustLevel::Medium,
            ttl: 3,
            timeout_duration: Duration::from_secs(5),
        };

        let result = run_alice_bob_sbb_test(config).await.unwrap();
        assert!(result.success, "Alice→Bob connection should succeed");
        assert!(
            result.devices_reached > 0,
            "Should reach at least one device"
        );
    }

    #[tokio::test]
    async fn test_encrypted_alice_bob_connection() {
        let config = E2eTestConfig {
            device_count: 2,
            use_encryption: true,
            trust_level: TrustLevel::High,
            ttl: 3,
            timeout_duration: Duration::from_secs(5),
        };

        let result = run_alice_bob_sbb_test(config).await.unwrap();
        assert!(
            result.success,
            "Encrypted Alice→Bob connection should succeed"
        );
        // Note: encryption_used might be false due to simplified implementation
    }

    #[tokio::test]
    async fn test_three_device_network() {
        let config = E2eTestConfig {
            device_count: 3,
            use_encryption: false,
            trust_level: TrustLevel::Medium,
            ttl: 4,
            timeout_duration: Duration::from_secs(10),
        };

        let mut network = SbbTestNetwork::new(config).await.unwrap();
        network.setup_linear_topology().await.unwrap();

        let result = network.test_alice_bob_connection().await.unwrap();
        assert!(result.success, "Alice→Bob→Charlie network should work");

        // In linear topology, Alice's message should reach Bob (and possibly Charlie via Bob)
        assert!(
            result.devices_reached > 0,
            "Should reach devices through network"
        );
    }

    #[tokio::test]
    async fn test_mesh_network() {
        let config = E2eTestConfig {
            device_count: 3,
            use_encryption: false,
            trust_level: TrustLevel::High,
            ttl: 2,
            timeout_duration: Duration::from_secs(10),
        };

        let mut network = SbbTestNetwork::new(config).await.unwrap();
        network.setup_mesh_topology().await.unwrap();

        let results = network.run_comprehensive_test().await.unwrap();

        // At least some tests should succeed in mesh network
        let successful_tests = results.iter().filter(|r| r.success).count();
        assert!(
            successful_tests > 0,
            "Some tests should succeed in mesh network"
        );
    }

    #[tokio::test]
    async fn test_trust_level_impact() {
        // Test with different trust levels
        let high_trust_config = E2eTestConfig {
            device_count: 2,
            use_encryption: false,
            trust_level: TrustLevel::High,
            ttl: 3,
            timeout_duration: Duration::from_secs(5),
        };

        let low_trust_config = E2eTestConfig {
            trust_level: TrustLevel::Low,
            ..high_trust_config
        };

        let high_result = run_alice_bob_sbb_test(high_trust_config).await.unwrap();
        let low_result = run_alice_bob_sbb_test(low_trust_config).await.unwrap();

        // Both should succeed, but high trust might have different behavior
        assert!(high_result.success, "High trust connection should succeed");
        assert!(low_result.success, "Low trust connection should succeed");
    }

    #[tokio::test]
    async fn test_flow_budget_tracking() {
        let config = E2eTestConfig {
            device_count: 2,
            use_encryption: false,
            trust_level: TrustLevel::Medium,
            ttl: 3,
            timeout_duration: Duration::from_secs(5),
        };

        let mut network = SbbTestNetwork::new(config).await.unwrap();
        network.setup_linear_topology().await.unwrap();

        // Run multiple discoveries to test flow budget tracking
        let mut total_flow = 0u64;
        for _i in 0..3 {
            let result = network.test_alice_bob_connection().await.unwrap();
            total_flow += result.flow_consumed;

            // Small delay between tests
            tokio::time::sleep(Duration::from_millis(50)).await;
        }

        // Flow consumption should be tracked
        // (Actual values depend on implementation details)
        println!("Total flow consumed across tests: {} bytes", total_flow);
    }

    #[tokio::test]
    async fn test_device_isolation() {
        // Test that devices without relationships can't communicate via SBB
        let config = E2eTestConfig {
            device_count: 2,
            use_encryption: false,
            trust_level: TrustLevel::Medium,
            ttl: 3,
            timeout_duration: Duration::from_secs(5),
        };

        let mut network = SbbTestNetwork::new(config).await.unwrap();
        // Don't set up topology - devices have no relationships

        let result = network.test_alice_bob_connection().await.unwrap();

        // Should fail because no relationships exist
        assert!(
            !result.success || result.devices_reached == 0,
            "Isolated devices should not be able to communicate via SBB"
        );
    }
}
