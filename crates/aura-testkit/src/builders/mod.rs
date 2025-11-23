//! Layer 8: Test Data Builders - Entity Factories & Scenarios
//!
//! Factories for creating test entities with consistent configuration:
//! **AccountBuilder**, **DeviceSetBuilder**, **KeySetBuilder**, **TestScenarioFactory**.
//!
//! **Purpose** (per docs/106_effect_system_and_runtime.md):
//! Enable rapid test setup without duplicating entity creation logic. Builders compose
//! to create complete multi-device scenarios for integration testing.

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
