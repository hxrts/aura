//! Integration tests for aura-sync protocols
//!
//! This module provides comprehensive integration tests for all aura-sync protocols
//! using aura-testkit's testing infrastructure. Tests cover realistic multi-device
//! scenarios including normal operation, failure modes, and recovery patterns.
//!
//! # Test Organization
//!
//! - `anti_entropy`: Anti-entropy sync under normal and adverse conditions
//! - `journal_sync`: Journal synchronization with divergent states
//! - `ota_coordination`: OTA coordination with threshold approval patterns
//! - `network_partition`: Network partition behavior and recovery
//! - `multi_device_scenarios`: Complex scenarios combining multiple protocols

// Sub-modules containing specific test scenarios
pub mod anti_entropy;
pub mod journal_sync;
pub mod multi_device_scenarios;
pub mod network_partition;
pub mod ota_coordination;

// Test utilities and helpers shared across integration tests
pub mod test_utils;

use aura_testkit::simulation::{
    choreography::{test_device_trio, ChoreographyTestHarness},
    network::NetworkSimulator,
};

pub use crate::shared_support::{
    test_device_id, test_session_manager, test_sync_config, test_time,
};

/// Create a test harness with three devices and mock network
pub async fn setup_test_trio() -> (ChoreographyTestHarness, NetworkSimulator) {
    let harness = test_device_trio();
    let network = NetworkSimulator::new();
    (harness, network)
}
