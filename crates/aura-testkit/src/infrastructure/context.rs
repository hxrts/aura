//! Foundational Test Infrastructure
//!
//! This module provides testing utilities that comply with the 8-layer architecture
//! by only depending on Layers 1-3 (Foundation, Specification, Implementation).
//!
//! # Architecture Compliance
//!
//! This module only imports from:
//! - Layer 1: aura-core (effect traits, types)
//! - Layer 2: domain crates (aura-journal, aura-transport)
//! - Layer 3: aura-effects (effect implementations)
//!
//! It does NOT import from:
//! - Layer 4+: aura-protocol, aura-agent, etc.
//!
//! # Usage
//!
//! ```rust,no_run
//! use aura_testkit::foundation::{TestEffectComposer, SimpleTestContext};
//! use aura_core::effects::{ExecutionMode, CryptoEffects, TimeEffects};
//!
//! // Create a test context with specific effect handlers
//! let context = SimpleTestContext::new(ExecutionMode::Testing)
//!     .with_mock_crypto()
//!     .with_mock_time()
//!     .build();
//!
//! // Use the context in tests with trait bounds
//! async fn test_crypto_operation<E: CryptoEffects>(effects: &E) {
//!     let key = effects.generate_signing_key().await.unwrap();
//!     // ... test logic
//! }
//! ```

use aura_core::{
    effects::{
        ConsoleEffects, CryptoEffects, ExecutionMode, JournalEffects, NetworkEffects,
        RandomEffects, StorageEffects, TimeEffects,
    },
    AuraResult, DeviceId,
};
// Foundation-based test infrastructure - no Arc needed for lightweight handlers

/// Simple test context that provides basic effect handler composition
///
/// This replaces the complex orchestration-layer effect runtime with a simple
/// composition suitable for testing foundational functionality.
pub struct SimpleTestContext {
    execution_mode: ExecutionMode,
    device_id: DeviceId,
}

impl SimpleTestContext {
    /// Create a new test context with the specified execution mode
    pub fn new(execution_mode: ExecutionMode) -> Self {
        Self {
            execution_mode,
            device_id: DeviceId::new(),
        }
    }

    /// Create a test context with a specific device ID
    pub fn with_device_id(execution_mode: ExecutionMode, device_id: DeviceId) -> Self {
        Self {
            execution_mode,
            device_id,
        }
    }

    /// Get the execution mode
    pub fn execution_mode(&self) -> ExecutionMode {
        self.execution_mode
    }

    /// Get the device ID
    pub fn device_id(&self) -> DeviceId {
        self.device_id
    }
}

/// Trait for composable test effect handlers
///
/// This trait allows tests to work with different effect handler combinations
/// without depending on the orchestration layer.
pub trait TestEffectHandler:
    CryptoEffects
    + NetworkEffects
    + StorageEffects
    + TimeEffects
    + RandomEffects
    + ConsoleEffects
    + JournalEffects
    + Send
    + Sync
{
    /// Get the execution mode for this handler
    fn execution_mode(&self) -> ExecutionMode;
}

/// Helper for creating test effect handlers from aura-effects implementations
pub struct TestEffectComposer {
    _execution_mode: ExecutionMode,
    _device_id: DeviceId,
}

impl TestEffectComposer {
    /// Create a new composer for the given execution mode
    pub fn new(execution_mode: ExecutionMode, device_id: DeviceId) -> Self {
        Self {
            _execution_mode: execution_mode,
            _device_id: device_id,
        }
    }

    /// Build a test effect handler using mock implementations
    pub fn build_mock_handler(&self) -> AuraResult<Box<dyn TestEffectHandler>> {
        // TODO: Implement using aura-effects mock handlers
        // This would compose MockCryptoHandler, MockNetworkHandler, etc.
        todo!("Implementation pending Task 5.3: Consolidate mock handler implementations")
    }

    /// Build a test effect handler using real implementations for integration tests
    pub fn build_real_handler(&self) -> AuraResult<Box<dyn TestEffectHandler>> {
        // TODO: Implement using aura-effects real handlers
        // This would compose RealCryptoHandler, RealNetworkHandler, etc.
        todo!("Implementation pending creation of real handler implementations in aura-effects")
    }
}

/// Convenience functions for common test scenarios

/// Create a simple mock effect context for unit tests
pub fn create_mock_test_context() -> AuraResult<SimpleTestContext> {
    Ok(SimpleTestContext::new(ExecutionMode::Testing))
}

/// Create a simulation context with deterministic behavior
pub fn create_simulation_context(seed: u64) -> AuraResult<SimpleTestContext> {
    Ok(SimpleTestContext::new(ExecutionMode::Simulation { seed }))
}

/// Create a production-like context for integration tests
pub fn create_integration_context() -> AuraResult<SimpleTestContext> {
    Ok(SimpleTestContext::new(ExecutionMode::Production))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_context_creation() {
        let context = SimpleTestContext::new(ExecutionMode::Testing);
        assert_eq!(context.execution_mode(), ExecutionMode::Testing);
        assert_ne!(context.device_id(), DeviceId::new()); // Should have unique ID
    }

    #[test]
    fn test_context_with_device_id() {
        let device_id = DeviceId::new();
        let context = SimpleTestContext::with_device_id(ExecutionMode::Testing, device_id);
        assert_eq!(context.execution_mode(), ExecutionMode::Testing);
        assert_eq!(context.device_id(), device_id);
    }

    #[test]
    fn test_convenience_functions() {
        let mock_context = create_mock_test_context().unwrap();
        assert_eq!(mock_context.execution_mode(), ExecutionMode::Testing);

        let sim_context = create_simulation_context(42).unwrap();
        assert_eq!(
            sim_context.execution_mode(),
            ExecutionMode::Simulation { seed: 42 }
        );

        let integration_context = create_integration_context().unwrap();
        assert_eq!(
            integration_context.execution_mode(),
            ExecutionMode::Production
        );
    }
}
