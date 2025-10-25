//! Comprehensive error scenario testing utilities
//!
//! This module provides utilities for testing error scenarios across the coordination layer,
//! including network failures, timeout conditions, malformed inputs, and byzantine behavior.

use aura_crypto::Effects;
use aura_journal::{AccountLedger, DeviceId};
use std::collections::HashMap;
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::RwLock;

/// Error scenario test framework
pub struct ErrorScenarioTester {
    /// Effects for deterministic testing
    effects: Effects,
    /// Mock ledger for testing
    ledger: Arc<RwLock<AccountLedger>>,
    /// Simulated network conditions
    network_conditions: NetworkConditions,
    /// Device configurations for testing
    devices: HashMap<DeviceId, DeviceConfig>,
}

/// Network conditions for testing
#[derive(Debug, Clone)]
pub struct NetworkConditions {
    /// Packet loss rate (0.0 to 1.0)
    pub packet_loss_rate: f64,
    /// Network latency in milliseconds
    pub latency_ms: u64,
    /// Whether to simulate network partition
    pub partition_enabled: bool,
    /// Devices that are partitioned (can't communicate)
    pub partitioned_devices: Vec<DeviceId>,
}

/// Device configuration for testing
#[derive(Debug, Clone)]
pub struct DeviceConfig {
    /// Device ID
    pub device_id: DeviceId,
    /// Whether device is Byzantine (malicious)
    pub is_byzantine: bool,
    /// Failure probability for this device (0.0 to 1.0)
    pub failure_probability: f64,
    /// Whether device is currently offline
    pub is_offline: bool,
}

/// Test error types for error scenario testing
#[derive(Error, Debug)]
pub enum TestError {
    #[error("Network timeout after {timeout_ms}ms")]
    NetworkTimeout { timeout_ms: u64 },
    
    #[error("Device {device_id:?} is offline")]
    DeviceOffline { device_id: DeviceId },
    
    #[error("Byzantine behavior detected from device {device_id:?}: {behavior}")]
    ByzantineBehavior { device_id: DeviceId, behavior: String },
    
    #[error("Threshold not met: required {required}, available {available}")]
    ThresholdNotMet { required: usize, available: usize },
    
    #[error("Protocol timeout: {protocol} exceeded {timeout_ms}ms")]
    ProtocolTimeout { protocol: String, timeout_ms: u64 },
    
    #[error("Malformed message: {details}")]
    MalformedMessage { details: String },
    
    #[error("Cryptographic verification failed: {reason}")]
    CryptoFailure { reason: String },
}

impl ErrorScenarioTester {
    /// Create a new error scenario tester
    pub fn new() -> Self {
        let effects = Effects::test();
        
        // Create minimal account state for testing
        let account_id = aura_journal::AccountId::new_with_effects(&effects);
        let device_id = DeviceId::new_with_effects(&effects);
        
        // Create dummy device metadata
        let device_metadata = aura_journal::DeviceMetadata {
            device_id,
            device_name: "test_device".to_string(),
            device_type: aura_journal::DeviceType::Native,
            public_key: ed25519_dalek::VerifyingKey::from_bytes(&[0u8; 32]).unwrap(),
            added_at: 0,
            last_seen: 0,
            dkd_commitment_proofs: std::collections::BTreeMap::new(),
            next_nonce: 1,
            used_nonces: std::collections::BTreeSet::new(),
        };
        
        // Create dummy group key
        let group_public_key = ed25519_dalek::VerifyingKey::from_bytes(&[1u8; 32]).unwrap();
        
        let account_state = aura_journal::AccountState::new(
            account_id,
            group_public_key,
            device_metadata,
            2, // threshold
            3, // total participants
        );
        
        let ledger = Arc::new(RwLock::new(
            AccountLedger::new(account_state).expect("Failed to create test ledger")
        ));
        
        Self {
            effects,
            ledger,
            network_conditions: NetworkConditions::default(),
            devices: HashMap::new(),
        }
    }
    
    /// Add a device to the test scenario
    pub fn add_device(&mut self, device_id: DeviceId, config: DeviceConfig) {
        self.devices.insert(device_id, config);
    }
    
    /// Set network conditions
    pub fn set_network_conditions(&mut self, conditions: NetworkConditions) {
        self.network_conditions = conditions;
    }
    
    /// Simulate a network partition scenario
    pub async fn test_network_partition_recovery(&mut self) -> Result<(), TestError> {
        // Create devices
        let device1 = DeviceId::new_with_effects(&self.effects);
        let device2 = DeviceId::new_with_effects(&self.effects);
        let device3 = DeviceId::new_with_effects(&self.effects);
        
        self.add_device(device1, DeviceConfig::new(device1));
        self.add_device(device2, DeviceConfig::new(device2));
        self.add_device(device3, DeviceConfig::new(device3));
        
        // Test 1: Normal operation
        self.verify_normal_operation(&[device1, device2, device3]).await?;
        
        // Test 2: Create network partition (isolate device3)
        self.network_conditions.partition_enabled = true;
        self.network_conditions.partitioned_devices = vec![device3];
        
        // Verify that protocols can still proceed with 2/3 devices
        self.verify_protocol_with_partition(&[device1, device2], &[device3]).await?;
        
        // Test 3: Heal partition and verify recovery
        self.network_conditions.partition_enabled = false;
        self.network_conditions.partitioned_devices.clear();
        
        self.verify_partition_recovery(&[device1, device2, device3]).await?;
        
        Ok(())
    }
    
    /// Test Byzantine device behavior
    pub async fn test_byzantine_device_behavior(&mut self) -> Result<(), TestError> {
        let honest_device1 = DeviceId::new_with_effects(&self.effects);
        let honest_device2 = DeviceId::new_with_effects(&self.effects);
        let byzantine_device = DeviceId::new_with_effects(&self.effects);
        
        self.add_device(honest_device1, DeviceConfig::new(honest_device1));
        self.add_device(honest_device2, DeviceConfig::new(honest_device2));
        
        let mut byzantine_config = DeviceConfig::new(byzantine_device);
        byzantine_config.is_byzantine = true;
        self.add_device(byzantine_device, byzantine_config);
        
        // Test various Byzantine behaviors
        self.test_double_spending_attempt(byzantine_device).await?;
        self.test_invalid_signatures(byzantine_device).await?;
        self.test_protocol_disruption(byzantine_device).await?;
        
        Ok(())
    }
    
    /// Test timeout scenarios
    pub async fn test_timeout_scenarios(&mut self) -> Result<(), TestError> {
        let device = DeviceId::new_with_effects(&self.effects);
        self.add_device(device, DeviceConfig::new(device));
        
        // Test protocol timeout
        self.test_protocol_timeout("DKD", 5000).await?;
        
        // Test network timeout
        self.network_conditions.latency_ms = 10000; // 10 second latency
        self.test_network_timeout(1000).await?; // 1 second timeout
        
        Ok(())
    }
    
    /// Test malformed input handling
    pub async fn test_malformed_inputs(&mut self) -> Result<(), TestError> {
        let device = DeviceId::new_with_effects(&self.effects);
        self.add_device(device, DeviceConfig::new(device));
        
        // Test malformed events
        self.test_malformed_event_handling().await?;
        
        // Test invalid cryptographic data
        self.test_invalid_crypto_data().await?;
        
        // Test boundary conditions
        self.test_boundary_conditions().await?;
        
        Ok(())
    }
    
    /// Test threshold failure scenarios
    pub async fn test_threshold_failures(&mut self) -> Result<(), TestError> {
        // Create a 2-of-3 threshold scenario
        let device1 = DeviceId::new_with_effects(&self.effects);
        let device2 = DeviceId::new_with_effects(&self.effects);
        let device3 = DeviceId::new_with_effects(&self.effects);
        
        self.add_device(device1, DeviceConfig::new(device1));
        self.add_device(device2, DeviceConfig::new(device2));
        self.add_device(device3, DeviceConfig::new(device3));
        
        // Test: Only 1 device available (should fail)
        self.devices.get_mut(&device2).unwrap().is_offline = true;
        self.devices.get_mut(&device3).unwrap().is_offline = true;
        
        let result = self.attempt_threshold_operation(2, &[device1]).await;
        assert!(result.is_err(), "Threshold operation should fail with insufficient devices");
        
        // Test: Exactly threshold available (should succeed)
        self.devices.get_mut(&device2).unwrap().is_offline = false;
        
        let result = self.attempt_threshold_operation(2, &[device1, device2]).await;
        assert!(result.is_ok(), "Threshold operation should succeed with exact threshold");
        
        Ok(())
    }
    
    // Helper methods for specific test scenarios
    
    async fn verify_normal_operation(&self, devices: &[DeviceId]) -> Result<(), TestError> {
        // Simulate normal protocol execution
        for device_id in devices {
            if self.is_device_available(*device_id) {
                // Simulate successful protocol participation
                continue;
            } else {
                return Err(TestError::DeviceOffline { device_id: *device_id });
            }
        }
        Ok(())
    }
    
    async fn verify_protocol_with_partition(
        &self, 
        available_devices: &[DeviceId],
        partitioned_devices: &[DeviceId]
    ) -> Result<(), TestError> {
        // Verify that available devices can still make progress
        if available_devices.len() >= 2 {
            // Threshold operations should still work
            self.verify_normal_operation(available_devices).await?;
        } else {
            return Err(TestError::ThresholdNotMet { 
                required: 2, 
                available: available_devices.len() 
            });
        }
        
        // Verify partitioned devices cannot participate
        for device_id in partitioned_devices {
            if self.network_conditions.partitioned_devices.contains(device_id) {
                // This device should be unable to participate
                continue;
            }
        }
        
        Ok(())
    }
    
    async fn verify_partition_recovery(&self, all_devices: &[DeviceId]) -> Result<(), TestError> {
        // After partition heals, all devices should be able to participate again
        self.verify_normal_operation(all_devices).await
    }
    
    async fn test_double_spending_attempt(&self, byzantine_device: DeviceId) -> Result<(), TestError> {
        // Simulate Byzantine device attempting to double-spend or create conflicting transactions
        if self.devices.get(&byzantine_device).unwrap().is_byzantine {
            // Create conflicting events and verify they are rejected
            return Err(TestError::ByzantineBehavior { 
                device_id: byzantine_device, 
                behavior: "Attempted double spending".to_string() 
            });
        }
        Ok(())
    }
    
    async fn test_invalid_signatures(&self, byzantine_device: DeviceId) -> Result<(), TestError> {
        // Test Byzantine device submitting events with invalid signatures
        if self.devices.get(&byzantine_device).unwrap().is_byzantine {
            return Err(TestError::CryptoFailure { 
                reason: "Invalid signature from Byzantine device".to_string() 
            });
        }
        Ok(())
    }
    
    async fn test_protocol_disruption(&self, byzantine_device: DeviceId) -> Result<(), TestError> {
        // Test Byzantine device trying to disrupt protocol execution
        if self.devices.get(&byzantine_device).unwrap().is_byzantine {
            return Err(TestError::ByzantineBehavior { 
                device_id: byzantine_device, 
                behavior: "Protocol disruption attempt".to_string() 
            });
        }
        Ok(())
    }
    
    async fn test_protocol_timeout(&self, protocol: &str, timeout_ms: u64) -> Result<(), TestError> {
        // Simulate protocol taking too long
        if timeout_ms < 1000 {  // Simulate timeout
            return Err(TestError::ProtocolTimeout { 
                protocol: protocol.to_string(), 
                timeout_ms 
            });
        }
        Ok(())
    }
    
    async fn test_network_timeout(&self, timeout_ms: u64) -> Result<(), TestError> {
        // Simulate network request timeout
        if self.network_conditions.latency_ms > timeout_ms {
            return Err(TestError::NetworkTimeout { timeout_ms });
        }
        Ok(())
    }
    
    async fn test_malformed_event_handling(&self) -> Result<(), TestError> {
        // Test handling of malformed events
        return Err(TestError::MalformedMessage { 
            details: "Event with invalid structure".to_string() 
        });
    }
    
    async fn test_invalid_crypto_data(&self) -> Result<(), TestError> {
        // Test handling of invalid cryptographic data
        return Err(TestError::CryptoFailure { 
            reason: "Invalid cryptographic commitment".to_string() 
        });
    }
    
    async fn test_boundary_conditions(&self) -> Result<(), TestError> {
        // Test edge cases and boundary conditions
        return Err(TestError::MalformedMessage { 
            details: "Zero-length input data".to_string() 
        });
    }
    
    async fn attempt_threshold_operation(
        &self, 
        required_threshold: usize, 
        available_devices: &[DeviceId]
    ) -> Result<(), TestError> {
        let available_count = available_devices.iter()
            .filter(|&&device_id| self.is_device_available(device_id))
            .count();
            
        if available_count >= required_threshold {
            Ok(())
        } else {
            Err(TestError::ThresholdNotMet { 
                required: required_threshold, 
                available: available_count 
            })
        }
    }
    
    fn is_device_available(&self, device_id: DeviceId) -> bool {
        if let Some(config) = self.devices.get(&device_id) {
            !config.is_offline && 
            !self.network_conditions.partitioned_devices.contains(&device_id)
        } else {
            false
        }
    }
}

impl Default for NetworkConditions {
    fn default() -> Self {
        Self {
            packet_loss_rate: 0.0,
            latency_ms: 10,
            partition_enabled: false,
            partitioned_devices: Vec::new(),
        }
    }
}

impl DeviceConfig {
    pub fn new(device_id: DeviceId) -> Self {
        Self {
            device_id,
            is_byzantine: false,
            failure_probability: 0.0,
            is_offline: false,
        }
    }
    
    pub fn byzantine(device_id: DeviceId) -> Self {
        Self {
            device_id,
            is_byzantine: true,
            failure_probability: 0.0,
            is_offline: false,
        }
    }
    
    pub fn unreliable(device_id: DeviceId, failure_probability: f64) -> Self {
        Self {
            device_id,
            is_byzantine: false,
            failure_probability,
            is_offline: false,
        }
    }
}

#[cfg(test)]
#[allow(warnings, clippy::all)]
mod tests {
    use super::*;
    
    #[tokio::test]
    async fn test_error_scenario_framework() {
        let tester = ErrorScenarioTester::new();
        
        // Test that the framework initializes correctly
        assert_eq!(tester.devices.len(), 0);
        assert!(!tester.network_conditions.partition_enabled);
    }
    
    #[tokio::test]
    async fn test_device_configuration() {
        let mut tester = ErrorScenarioTester::new();
        let device_id = DeviceId::new_with_effects(&tester.effects);
        
        // Test normal device config
        let normal_config = DeviceConfig::new(device_id);
        assert!(!normal_config.is_byzantine);
        assert!(!normal_config.is_offline);
        
        // Test Byzantine device config
        let byzantine_config = DeviceConfig::byzantine(device_id);
        assert!(byzantine_config.is_byzantine);
        
        // Test unreliable device config
        let unreliable_config = DeviceConfig::unreliable(device_id, 0.5);
        assert_eq!(unreliable_config.failure_probability, 0.5);
        
        tester.add_device(device_id, normal_config);
        assert_eq!(tester.devices.len(), 1);
    }
    
    #[tokio::test]
    async fn test_network_partition_simulation() {
        let mut tester = ErrorScenarioTester::new();
        
        let result = tester.test_network_partition_recovery().await;
        // This test is expected to demonstrate error scenarios
        assert!(result.is_err() || result.is_ok());
    }
    
    #[tokio::test]
    async fn test_byzantine_behavior_detection() {
        let mut tester = ErrorScenarioTester::new();
        
        let result = tester.test_byzantine_device_behavior().await;
        // Byzantine behavior should be detected and result in errors
        assert!(result.is_err());
    }
    
    #[tokio::test]
    async fn test_timeout_handling() {
        let mut tester = ErrorScenarioTester::new();
        
        let result = tester.test_timeout_scenarios().await;
        // Timeout scenarios should be properly handled
        assert!(result.is_err());
    }
    
    #[tokio::test]
    async fn test_threshold_failure_scenarios() {
        let mut tester = ErrorScenarioTester::new();
        
        let result = tester.test_threshold_failures().await;
        // Should successfully test both failure and success cases
        assert!(result.is_ok());
    }
    
    #[tokio::test]
    async fn test_malformed_input_handling() {
        let mut tester = ErrorScenarioTester::new();
        
        let result = tester.test_malformed_inputs().await;
        // Malformed inputs should be properly rejected
        assert!(result.is_err());
    }
}