//! Test harness utilities for Aura tests
//!
//! This module provides the runtime support for the `#[aura_test]` macro,
//! including automatic setup/teardown, test isolation, and utility functions.

use std::sync::Once;
use std::time::Duration;

use crate::foundation::{create_mock_test_context, SimpleTestContext};
use aura_agent::runtime::{AuraEffectSystem, EffectSystemConfig};
use aura_core::{AuraError, AuraResult, DeviceId};
use std::sync::Arc;
use tracing_subscriber::EnvFilter;

static TRACING_INIT: Once = Once::new();

/// Initialize test tracing
///
/// This sets up tracing for tests with appropriate filtering.
/// It's safe to call multiple times - initialization only happens once.
pub fn init_test_tracing() -> TestTracingGuard {
    TRACING_INIT.call_once(|| {
        let filter =
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("aura=debug,warn"));

        tracing_subscriber::fmt()
            .with_env_filter(filter)
            .with_test_writer()
            .with_target(false)
            .with_level(true)
            .with_thread_ids(true)
            .with_thread_names(true)
            .try_init()
            .ok(); // Ignore error if already initialized
    });

    TestTracingGuard
}

/// Guard that ensures tracing stays active for the test duration
pub struct TestTracingGuard;

/// Test context providing access to initialized test context
pub struct TestContext {
    /// The foundation-based test context for this test
    pub context: SimpleTestContext,
    /// Test-specific configuration
    pub config: TestConfig,
}

/// Configuration for test execution
#[derive(Clone, Debug)]
pub struct TestConfig {
    /// Test name for identification
    pub name: String,
    /// Whether to use deterministic time
    pub deterministic_time: bool,
    /// Whether to capture effects
    pub capture_effects: bool,
    /// Test timeout
    pub timeout: Option<Duration>,
}

impl Default for TestConfig {
    fn default() -> Self {
        Self {
            name: "test".to_string(),
            deterministic_time: true,
            capture_effects: false,
            timeout: Some(Duration::from_secs(30)),
        }
    }
}

/// Create a test context with default configuration
pub async fn create_test_context() -> AuraResult<TestContext> {
    create_test_context_with_config(TestConfig::default()).await
}

/// Create a test context with custom configuration
pub async fn create_test_context_with_config(config: TestConfig) -> AuraResult<TestContext> {
    let context = create_mock_test_context()?;

    Ok(TestContext { context, config })
}

/// Test fixture for common test scenarios
pub struct TestFixture {
    context: TestContext,
}

impl TestFixture {
    /// Create a new test fixture
    pub async fn new() -> AuraResult<Self> {
        let context = create_test_context().await?;
        Ok(Self { context })
    }

    /// Create a fixture with custom configuration
    pub async fn with_config(config: TestConfig) -> AuraResult<Self> {
        let context = create_test_context_with_config(config).await?;
        Ok(Self { context })
    }

    /// Get the test context
    pub fn context(&self) -> &SimpleTestContext {
        &self.context.context
    }

    /// Get the device ID
    pub fn device_id(&self) -> DeviceId {
        self.context.context.device_id()
    }

    /// Create a new device ID for testing
    ///
    /// This is a convenience method that simply creates a new random DeviceId.
    /// Tests should use this when they need fresh device identifiers.
    pub fn create_device_id(&self) -> DeviceId {
        DeviceId::new()
    }

    /// Get an effect system for testing
    ///
    /// This creates a mock effect system configured for testing.
    pub fn effect_system(&self) -> Arc<AuraEffectSystem> {
        let config = EffectSystemConfig::for_testing(self.device_id());
        let system = AuraEffectSystem::new(config).expect("Failed to create test effect system");
        Arc::new(system)
    }

    /// Get a reference to the test context for effect access
    ///
    /// Note: The architecture has shifted to stateless effect handlers.
    /// Tests should create handlers directly rather than using a centralized
    /// effect system. This method returns the context for compatibility.
    pub fn effects(&self) -> &SimpleTestContext {
        &self.context.context
    }

    /// Run a test with automatic cleanup
    pub async fn run_test<F, Fut, T>(&self, test_fn: F) -> AuraResult<T>
    where
        F: FnOnce(&SimpleTestContext) -> Fut,
        Fut: std::future::Future<Output = AuraResult<T>>,
    {
        // Run the test
        let result = test_fn(&self.context.context).await;

        // Cleanup is handled by Drop implementations

        result
    }
}

/// Drop implementation ensures cleanup
impl Drop for TestFixture {
    fn drop(&mut self) {
        // Foundation-based test context has lightweight cleanup
        // No async shutdown needed for stateless handlers
    }
}

/// Snapshot testing support
pub mod snapshot {
    use super::*;
    use std::collections::HashMap;

    /// Effect snapshot for testing
    #[derive(Debug, Clone)]
    pub struct EffectSnapshot {
        /// Captured effect calls
        pub calls: Vec<EffectCall>,
        /// Metadata about the snapshot
        pub metadata: HashMap<String, String>,
    }

    /// A single captured effect call
    #[derive(Debug, Clone)]
    pub struct EffectCall {
        /// Effect type
        pub effect_type: String,
        /// Operation name
        pub operation: String,
        /// Serialized parameters
        pub params: Vec<u8>,
        /// Timestamp relative to test start
        pub timestamp: Duration,
    }

    impl EffectSnapshot {
        /// Create a new empty snapshot
        pub fn new() -> Self {
            Self {
                calls: Vec::new(),
                metadata: HashMap::new(),
            }
        }

        /// Record an effect call
        pub fn record(&mut self, call: EffectCall) {
            self.calls.push(call);
        }

        /// Assert snapshot matches expected
        pub fn assert_matches(&self, expected: &EffectSnapshot) -> AuraResult<()> {
            // Simple implementation - compare counts
            if self.calls.len() != expected.calls.len() {
                return Err(AuraError::invalid(format!(
                    "Snapshot mismatch: expected {} calls, got {}",
                    expected.calls.len(),
                    self.calls.len()
                )));
            }

            // Compare each call
            for (i, (actual, expected)) in self.calls.iter().zip(expected.calls.iter()).enumerate()
            {
                if actual.effect_type != expected.effect_type
                    || actual.operation != expected.operation
                {
                    return Err(AuraError::invalid(format!(
                        "Call {} mismatch: expected {}::{}, got {}::{}",
                        i,
                        expected.effect_type,
                        expected.operation,
                        actual.effect_type,
                        actual.operation
                    )));
                }
            }

            Ok(())
        }
    }
}
