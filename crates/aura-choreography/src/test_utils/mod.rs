//! Test utilities for choreographic protocols

use aura_protocol::effects::choreographic::ChoreographicRole;
use aura_protocol::effects::Effects;
use uuid::Uuid;

pub mod crypto_test_utils;
pub mod scenario_runner;

/// Create a test role for choreographic protocols
pub fn create_test_role(index: usize) -> ChoreographicRole {
    ChoreographicRole {
        device_id: Uuid::new_v4(),
        role_index: index,
    }
}

/// Create test participants for protocols
pub fn create_test_participants(count: usize) -> Vec<ChoreographicRole> {
    (0..count).map(create_test_role).collect()
}

/// Create test effects for deterministic testing
pub fn create_test_effects(seed: u64) -> Effects {
    Effects::deterministic(seed, 0)
}

// TODO: Add choreographic adapters and endpoints once the runtime is implemented