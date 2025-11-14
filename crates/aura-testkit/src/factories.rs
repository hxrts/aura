//! Test data factories
//!
//! This module provides factories for creating complex test data structures
//! with consistent configuration across the Aura test suite.

use crate::device::DeviceSetBuilder;
use aura_core::hash::hash;
use aura_core::{AccountId, DeviceId};
use aura_journal::semilattice::ModernAccountState as AccountState;
use aura_journal::{DeviceMetadata, DeviceType};
use uuid::Uuid;

/// Factory for creating complete test scenarios with consistent configuration
#[derive(Debug)]
pub struct TestScenarioFactory {
    scenario_name: String,
    device_count: usize,
    threshold: u16,
    base_seed: Option<u64>,
}

impl TestScenarioFactory {
    /// Create a new test scenario factory
    pub fn new(scenario_name: String) -> Self {
        Self {
            scenario_name,
            device_count: 1,
            threshold: 1,
            base_seed: None,
        }
    }

    /// Set the number of devices in the scenario
    pub fn with_devices(mut self, count: usize) -> Self {
        self.device_count = count;
        self
    }

    /// Set the threshold for threshold cryptography
    pub fn with_threshold(mut self, threshold: u16) -> Self {
        self.threshold = threshold;
        self
    }

    /// Set a base seed for deterministic generation
    pub fn with_seed(mut self, seed: u64) -> Self {
        self.base_seed = Some(seed);
        self
    }

    /// Get scenario configuration
    pub fn config(&self) -> TestScenarioConfig {
        TestScenarioConfig {
            scenario_name: self.scenario_name.clone(),
            device_count: self.device_count,
            threshold: self.threshold,
            base_seed: self.base_seed,
        }
    }
}

/// Test scenario configuration
#[derive(Debug, Clone)]
pub struct TestScenarioConfig {
    /// Name of the test scenario
    pub scenario_name: String,
    /// Number of devices in the scenario
    pub device_count: usize,
    /// Threshold for multi-signature operations
    pub threshold: u16,
    /// Optional seed for deterministic randomness
    pub base_seed: Option<u64>,
}

impl TestScenarioConfig {
    /// Get configuration for a standard scenario
    pub fn standard(scenario_name: &str) -> Self {
        match scenario_name {
            "single-device" => Self {
                scenario_name: scenario_name.to_string(),
                device_count: 1,
                threshold: 1,
                base_seed: Some(42),
            },
            "dual-device" => Self {
                scenario_name: scenario_name.to_string(),
                device_count: 2,
                threshold: 1,
                base_seed: Some(43),
            },
            "three-device" => Self {
                scenario_name: scenario_name.to_string(),
                device_count: 3,
                threshold: 2,
                base_seed: Some(44),
            },
            "threshold-2-3" => Self {
                scenario_name: scenario_name.to_string(),
                device_count: 3,
                threshold: 2,
                base_seed: Some(45),
            },
            "threshold-3-5" => Self {
                scenario_name: scenario_name.to_string(),
                device_count: 5,
                threshold: 3,
                base_seed: Some(46),
            },
            "distributed-4" => Self {
                scenario_name: scenario_name.to_string(),
                device_count: 4,
                threshold: 2,
                base_seed: Some(47),
            },
            _ => Self {
                scenario_name: scenario_name.to_string(),
                device_count: 1,
                threshold: 1,
                base_seed: Some(42),
            },
        }
    }
}

/// Account state factory for creating test accounts
#[derive(Debug)]
pub struct AccountStateFactory {
    account_id: AccountId,
    devices: Vec<DeviceMetadata>,
}

impl AccountStateFactory {
    /// Create a new account state factory
    pub fn new(account_id: AccountId) -> Self {
        Self {
            account_id,
            devices: vec![],
        }
    }

    /// Create a random account state factory
    pub fn random() -> Self {
        // Deterministic UUID generation
        let hash_input = "random-account-factory";
        let hash_bytes = hash(hash_input.as_bytes());
        let uuid = Uuid::from_bytes(hash_bytes[..16].try_into().unwrap());
        Self::new(AccountId(uuid))
    }

    /// Add a device to the account state
    pub async fn add_device(mut self, device_id: DeviceId, device_type: DeviceType) -> Self {
        // Use simple deterministic approach for factory
        let (_signing_key, public_key) = crate::test_key_pair(42);
        let timestamp = 1000; // Default test timestamp

        let metadata = DeviceMetadata {
            device_id,
            device_name: format!("Device {:?}", device_id),
            device_type,
            public_key,
            added_at: timestamp,
            last_seen: timestamp,
            dkd_commitment_proofs: Default::default(),
            next_nonce: 0,
            key_share_epoch: 0,
            used_nonces: Default::default(),
        };
        self.devices.push(metadata);
        self
    }

    /// Add multiple devices of a specific type
    pub async fn add_devices(mut self, count: usize, device_type: DeviceType) -> Self {
        for _ in 0..count {
            let device_id = DeviceId::new();
            self = self.add_device(device_id, device_type).await;
        }
        self
    }

    /// Build the account state
    pub async fn build(self) -> AccountState {
        // Create AccountState using the new constructor
        // We need at least one device
        let initial_device = if let Some(first_device) = self.devices.first() {
            first_device.clone()
        } else {
            // Create a default device if none exists
            let (_, public_key) = crate::test_key_pair(42);
            let timestamp = 1000; // Default test timestamp

            DeviceMetadata {
                device_id: DeviceId::new(),
                device_name: "Default Device".to_string(),
                device_type: DeviceType::Native,
                public_key,
                added_at: timestamp,
                last_seen: timestamp,
                dkd_commitment_proofs: Default::default(),
                next_nonce: 0,
                key_share_epoch: 0,
                used_nonces: Default::default(),
            }
        };

        // Use a default group key from the first device
        let group_public_key = initial_device.public_key;

        let mut state = AccountState::new(self.account_id, group_public_key);

        // Add the initial device and all remaining devices
        state.add_device(initial_device);
        for device in self.devices.iter().skip(1) {
            state.add_device(device.clone());
        }

        state
    }
}

/// Coordinated test data factory for multi-device scenarios
#[derive(Debug)]
pub struct MultiDeviceScenarioFactory {
    account_id: AccountId,
    base_seed: u64,
    device_count: usize,
    threshold: u16,
}

impl MultiDeviceScenarioFactory {
    /// Create a new multi-device scenario factory
    pub fn new(device_count: usize, threshold: u16) -> Self {
        // Deterministic UUID generation
        let hash_input = format!("scenario-factory-{}-{}", device_count, threshold);
        let hash_bytes = hash(hash_input.as_bytes());
        let uuid = Uuid::from_bytes(hash_bytes[..16].try_into().unwrap());
        Self {
            account_id: AccountId(uuid),
            base_seed: 42,
            device_count,
            threshold,
        }
    }

    /// Set a custom account ID
    pub fn with_account_id(mut self, account_id: AccountId) -> Self {
        self.account_id = account_id;
        self
    }

    /// Set a custom base seed
    pub fn with_seed(mut self, seed: u64) -> Self {
        self.base_seed = seed;
        self
    }

    /// Build all test data components
    pub async fn build(self) -> MultiDeviceScenarioData {
        // Build devices
        let devices = DeviceSetBuilder::new(self.device_count)
            .with_seed(self.base_seed)
            .build();

        // Build account state with all devices
        let mut account_builder = AccountStateFactory::new(self.account_id);
        for (i, device) in devices.iter().enumerate() {
            let device_type = if i == 0 {
                DeviceType::Native
            } else {
                DeviceType::Browser
            };
            account_builder = account_builder
                .add_device(device.device_id(), device_type)
                .await;
        }

        let account_state = account_builder.build().await;

        MultiDeviceScenarioData {
            account_id: self.account_id,
            devices,
            account_state,
            threshold: self.threshold,
        }
    }
}

/// Complete multi-device scenario data
#[derive(Debug, Clone)]
pub struct MultiDeviceScenarioData {
    /// Account identifier for the scenario
    pub account_id: AccountId,
    /// Device fixtures in the scenario
    pub devices: Vec<crate::device::DeviceTestFixture>,
    /// Current account state
    pub account_state: AccountState,
    /// Threshold for multi-signature operations
    pub threshold: u16,
}

/// Common test data factory helpers
pub mod helpers {
    use super::*;

    /// Create a standard test scenario
    pub fn standard_scenario(scenario_name: &str) -> TestScenarioConfig {
        TestScenarioConfig::standard(scenario_name)
    }

    /// Create test data for a single-device scenario
    pub fn single_device_scenario() -> TestScenarioConfig {
        TestScenarioConfig::standard("single-device")
    }

    /// Create test data for a dual-device scenario
    pub fn dual_device_scenario() -> TestScenarioConfig {
        TestScenarioConfig::standard("dual-device")
    }

    /// Create test data for a three-device scenario
    pub fn three_device_scenario() -> TestScenarioConfig {
        TestScenarioConfig::standard("three-device")
    }

    /// Create test data for threshold-based scenarios
    pub fn threshold_scenario(threshold: u16, total: usize) -> TestScenarioConfig {
        TestScenarioConfig {
            scenario_name: format!("threshold-{}-{}", threshold, total),
            device_count: total,
            threshold,
            base_seed: Some(100 + threshold as u64),
        }
    }

    /// Create a complete multi-device scenario
    pub async fn multi_device_scenario(
        device_count: usize,
        threshold: u16,
    ) -> MultiDeviceScenarioData {
        MultiDeviceScenarioFactory::new(device_count, threshold)
            .build()
            .await
    }

    /// Create a multi-device scenario with custom account
    pub async fn multi_device_scenario_with_account(
        account_id: AccountId,
        device_count: usize,
        threshold: u16,
    ) -> MultiDeviceScenarioData {
        MultiDeviceScenarioFactory::new(device_count, threshold)
            .with_account_id(account_id)
            .build()
            .await
    }

    /// Get all available scenario names
    pub fn available_scenarios() -> Vec<&'static str> {
        vec![
            "single-device",
            "dual-device",
            "three-device",
            "threshold-2-3",
            "threshold-3-5",
            "distributed-4",
        ]
    }

    /// Verify scenario data integrity
    pub fn verify_scenario_integrity(data: &MultiDeviceScenarioData) -> bool {
        // Verify device count matches
        if data.devices.len() != data.account_state.device_registry.devices.len() {
            return false;
        }

        // Verify all devices are present in account state
        for device in &data.devices {
            if !data
                .account_state
                .device_registry
                .devices
                .iter()
                .any(|(_, metadata)| metadata.device_id == device.device_id())
            {
                return false;
            }
        }

        // Verify threshold is valid
        if data.threshold == 0 || data.threshold > data.devices.len() as u16 {
            return false;
        }

        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scenario_config_creation() {
        let config = TestScenarioConfig::standard("three-device");
        assert_eq!(config.device_count, 3);
        assert_eq!(config.threshold, 2);
    }

    #[tokio::test]
    async fn test_account_state_factory() {
        let account_id = AccountId::new();
        let state = AccountStateFactory::new(account_id)
            .add_device(DeviceId::new(), DeviceType::Native)
            .await
            .add_device(DeviceId::new(), DeviceType::Browser)
            .await
            .build()
            .await;

        assert_eq!(state.device_registry.devices.len(), 2);
    }

    #[tokio::test]
    async fn test_multi_device_scenario_factory() {
        let scenario = MultiDeviceScenarioFactory::new(3, 2).build().await;

        assert_eq!(scenario.devices.len(), 3);
        assert_eq!(scenario.threshold, 2);
        assert_eq!(scenario.account_state.device_registry.devices.len(), 3);
        assert!(helpers::verify_scenario_integrity(&scenario));
    }

    #[test]
    fn test_scenario_helpers() {
        let single = helpers::single_device_scenario();
        assert_eq!(single.device_count, 1);

        let dual = helpers::dual_device_scenario();
        assert_eq!(dual.device_count, 2);

        let three = helpers::three_device_scenario();
        assert_eq!(three.device_count, 3);
    }

    #[test]
    fn test_available_scenarios() {
        let scenarios = helpers::available_scenarios();
        assert!(!scenarios.is_empty());
        assert!(scenarios.contains(&"single-device"));
        assert!(scenarios.contains(&"threshold-3-5"));
    }

    #[tokio::test]
    async fn test_scenario_integrity_verification() {
        let scenario = helpers::multi_device_scenario(5, 3).await;
        assert!(helpers::verify_scenario_integrity(&scenario));
    }
}
