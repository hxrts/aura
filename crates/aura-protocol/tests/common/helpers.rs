//! Test helper utilities for aura-protocol tests
//!
//! This module provides common test utilities to reduce duplication across
//! test modules and standardize test patterns.
//!
//! NOTE: Many functions are disabled as they depend on modules not yet implemented.

use aura_core::{AccountId, DeviceId, SessionId};
use aura_protocol::{
    handlers::CompositeHandler,
    // Note: middleware and runtime modules not yet fully implemented
    // middleware::{MiddlewareConfig, create_standard_stack},
    // runtime::{ExecutionContext, ContextBuilder},
    // effects::ProtocolEffects,
};
use uuid::Uuid;

/// Create a test DeviceId
#[allow(dead_code)]
pub fn create_test_device_id() -> DeviceId {
    DeviceId::from(Uuid::from_u128(12345))
}

/// Create a second test DeviceId for multi-device tests
#[allow(dead_code)]
pub fn create_test_device_id_2() -> DeviceId {
    DeviceId::from(Uuid::from_u128(67890))
}

/// Create a test SessionId
#[allow(dead_code)]
pub fn create_test_session_id() -> SessionId {
    SessionId::from(Uuid::from_u128(11111))
}

/// Create a test AccountId
#[allow(dead_code)]
pub fn create_test_account_id() -> AccountId {
    AccountId::from_uuid(Uuid::from_u128(22222))
}

/// Create a list of test participants
#[allow(dead_code)]
pub fn create_test_participants(count: usize) -> Vec<DeviceId> {
    (0..count)
        .map(|i| DeviceId::from(Uuid::from_u128(1000 + i as u128)))
        .collect()
}

/// Create a composite handler for testing
#[allow(dead_code)]
pub fn create_test_handler() -> CompositeHandler {
    CompositeHandler::for_testing(create_test_device_id().into())
}

/// Create a composite handler for simulation
#[allow(dead_code)]
pub fn create_simulation_handler() -> CompositeHandler {
    CompositeHandler::for_simulation(create_test_device_id().into())
}

/// Create deterministic UUIDs for testing
#[allow(dead_code)]
pub fn create_deterministic_uuid(seed: u128) -> Uuid {
    Uuid::from_u128(seed + 0x1234567890abcdef)
}

/// Create test data of specified size
#[allow(dead_code)]
pub fn create_test_data(size: usize) -> Vec<u8> {
    (0..size).map(|i| (i % 256) as u8).collect()
}

/// Create a test keypair (returns dummy values TODO fix - For now)
#[allow(dead_code)]
pub fn create_test_keypair() -> ([u8; 32], [u8; 32]) {
    let private_key = [0u8; 32];
    let public_key = [1u8; 32];
    (private_key, public_key)
}

// === DISABLED FUNCTIONS (depend on missing modules) ===
//
// /// Create a test execution context
// pub fn create_test_execution_context() -> ExecutionContext { ... }
//
// /// Create a test execution context for simulation
// pub fn create_test_simulation_context() -> ExecutionContext { ... }
//
// /// Create a middleware config for testing
// pub fn create_test_middleware_config() -> MiddlewareConfig { ... }
//
// /// Create a handler with middleware for testing
// pub fn create_test_handler_with_middleware() -> impl ProtocolEffects { ... }
