//! Device test helpers and utilities
//!
//! This module provides standardized helpers for creating and managing test devices
//! across the Aura test suite.

use aura_core::hash::hash;
use aura_core::DeviceId;
use uuid::Uuid;

/// Device test fixture for consistent test device creation
#[derive(Debug, Clone)]
pub struct DeviceTestFixture {
    device_id: DeviceId,
    index: usize,
    label: String,
}

impl DeviceTestFixture {
    /// Create a new device test fixture with a specific index
    pub fn new(index: usize) -> Self {
        // Deterministic UUID based on index
        let hash_input = format!("device-fixture-{}", index);
        let hash_bytes = hash(hash_input.as_bytes());
        let uuid = Uuid::from_bytes(hash_bytes[..16].try_into().unwrap());
        let device_id = DeviceId(uuid);
        Self {
            device_id,
            index,
            label: format!("device_{}", index),
        }
    }

    /// Create a device fixture with a specific UUID
    pub fn with_id(id: Uuid, index: usize) -> Self {
        let device_id = DeviceId(id);
        Self {
            device_id,
            index,
            label: format!("device_{}", index),
        }
    }

    /// Create a device fixture with a custom label
    pub fn with_label(index: usize, label: String) -> Self {
        // Deterministic UUID based on index
        let hash_input = format!("device-fixture-{}", index);
        let hash_bytes = hash(hash_input.as_bytes());
        let uuid = Uuid::from_bytes(hash_bytes[..16].try_into().unwrap());
        let device_id = DeviceId(uuid);
        Self {
            device_id,
            index,
            label,
        }
    }

    /// Get the device ID
    pub fn device_id(&self) -> DeviceId {
        self.device_id
    }

    /// Get the device index (for ordering)
    pub fn index(&self) -> usize {
        self.index
    }

    /// Get the device label
    pub fn label(&self) -> &str {
        &self.label
    }

    /// Get a reference to the raw UUID
    pub fn uuid(&self) -> Uuid {
        self.device_id.0
    }
}

/// Simple helper to create a deterministic test DeviceId from an index
///
/// This is a convenience function for tests that need quick DeviceId creation.
/// For more complex device setup, use `DeviceTestFixture` instead.
pub fn test_device_id(index: usize) -> DeviceId {
    DeviceTestFixture::new(index).device_id()
}

/// Builder for creating multiple test devices with consistent configuration
#[derive(Debug)]
pub struct DeviceSetBuilder {
    count: usize,
    base_seed: Option<u64>,
    labels: Option<Vec<String>>,
}

impl DeviceSetBuilder {
    /// Create a new device set builder
    pub fn new(count: usize) -> Self {
        Self {
            count,
            base_seed: None,
            labels: None,
        }
    }

    /// Set a base seed for deterministic device ID generation
    pub fn with_seed(mut self, seed: u64) -> Self {
        self.base_seed = Some(seed);
        self
    }

    /// Set custom labels for devices
    pub fn with_labels(mut self, labels: Vec<String>) -> Self {
        self.labels = Some(labels);
        self
    }

    /// Build the set of devices
    pub fn build(self) -> Vec<DeviceTestFixture> {
        (0..self.count)
            .map(|i| {
                let label = self
                    .labels
                    .as_ref()
                    .and_then(|l| l.get(i).cloned())
                    .unwrap_or_else(|| format!("device_{}", i));

                let device_id = if let Some(seed) = self.base_seed {
                    // Deterministic generation from seed
                    let hash_input = format!("{}-{}", seed, i);
                    let hash_bytes = hash(hash_input.as_bytes());
                    let uuid_bytes: [u8; 16] = hash_bytes[..16].try_into().unwrap();
                    DeviceId(Uuid::from_bytes(uuid_bytes))
                } else {
                    // Deterministic generation based on index
                    let hash_input = format!("device-set-{}", i);
                    let hash_bytes = hash(hash_input.as_bytes());
                    let uuid_bytes: [u8; 16] = hash_bytes[..16].try_into().unwrap();
                    DeviceId(Uuid::from_bytes(uuid_bytes))
                };

                DeviceTestFixture {
                    device_id,
                    index: i,
                    label,
                }
            })
            .collect()
    }
}

/// Common test device creation helpers
pub mod helpers {
    use super::*;

    /// Create a single test device with default configuration
    pub fn test_device() -> DeviceTestFixture {
        DeviceTestFixture::new(0)
    }

    /// Create N test devices with sequential indexing
    pub fn test_devices(count: usize) -> Vec<DeviceTestFixture> {
        (0..count).map(DeviceTestFixture::new).collect()
    }

    /// Create test devices with specific labels
    pub fn test_devices_with_labels(labels: Vec<&str>) -> Vec<DeviceTestFixture> {
        labels
            .into_iter()
            .enumerate()
            .map(|(i, label)| DeviceTestFixture::with_label(i, label.to_string()))
            .collect()
    }

    /// Create a device pair for two-party tests
    pub fn test_device_pair() -> (DeviceTestFixture, DeviceTestFixture) {
        (DeviceTestFixture::new(0), DeviceTestFixture::new(1))
    }

    /// Create three devices for three-party tests
    pub fn test_device_trio() -> (DeviceTestFixture, DeviceTestFixture, DeviceTestFixture) {
        (
            DeviceTestFixture::new(0),
            DeviceTestFixture::new(1),
            DeviceTestFixture::new(2),
        )
    }

    /// Create deterministic devices from a seed
    pub fn test_devices_seeded(count: usize, seed: u64) -> Vec<DeviceTestFixture> {
        DeviceSetBuilder::new(count).with_seed(seed).build()
    }

    /// Get common test device labels
    pub fn standard_labels() -> Vec<&'static str> {
        vec!["alice", "bob", "charlie", "diana", "eve"]
    }

    /// Verify device collection integrity
    pub fn verify_device_uniqueness(devices: &[DeviceTestFixture]) -> bool {
        let mut seen = std::collections::HashSet::new();
        devices.iter().all(|d| seen.insert(d.device_id()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_device_fixture_creation() {
        let device = DeviceTestFixture::new(0);
        assert_eq!(device.index(), 0);
        assert_eq!(device.label(), "device_0");
    }

    #[test]
    fn test_device_set_builder() {
        let devices = DeviceSetBuilder::new(3).build();
        assert_eq!(devices.len(), 3);
        assert!(helpers::verify_device_uniqueness(&devices));
    }

    #[test]
    fn test_device_set_with_labels() {
        let labels = vec![
            "alice".to_string(),
            "bob".to_string(),
            "charlie".to_string(),
        ];
        let devices = DeviceSetBuilder::new(3).with_labels(labels).build();
        assert_eq!(devices[0].label(), "alice");
        assert_eq!(devices[1].label(), "bob");
        assert_eq!(devices[2].label(), "charlie");
    }

    #[test]
    fn test_seeded_device_generation_deterministic() {
        let devices1 = helpers::test_devices_seeded(3, 42);
        let devices2 = helpers::test_devices_seeded(3, 42);

        for (d1, d2) in devices1.iter().zip(devices2.iter()) {
            assert_eq!(d1.device_id(), d2.device_id());
        }
    }

    #[test]
    fn test_device_helpers() {
        let (d1, d2) = helpers::test_device_pair();
        assert_eq!(d1.index(), 0);
        assert_eq!(d2.index(), 1);

        let (d1, d2, d3) = helpers::test_device_trio();
        assert_eq!(d1.index(), 0);
        assert_eq!(d2.index(), 1);
        assert_eq!(d3.index(), 2);
    }
}
