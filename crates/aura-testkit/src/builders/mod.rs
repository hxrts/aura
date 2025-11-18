//! Test data construction & factories
//!
//! This module provides factories for creating individual test entities (accounts, devices, keys)
//! with consistent configuration across the Aura test suite. These are the lowest-level building
//! blocks for constructing test scenarios.

pub mod account;
pub mod device;
pub mod factories;
pub mod keys;

// Re-export builders without glob re-exports to avoid ambiguity with helpers submodules
pub use account::*;
pub use device::{DeviceSetBuilder, DeviceTestFixture};
pub use factories::{
    JournalFactory, MultiDeviceScenarioData, MultiDeviceScenarioFactory, TestScenarioConfig,
    TestScenarioFactory,
};
pub use keys::{KeySetBuilder, KeyTestFixture, KeyType};
