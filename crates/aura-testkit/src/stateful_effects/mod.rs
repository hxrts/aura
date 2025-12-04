//! # Stateful Effect Handlers for Testing
//!
//! This module contains stateful effect handlers that are designed for testing purposes.
//! These handlers were moved from aura-effects to fix architectural violations where
//! production effect handlers contained shared mutable state (Arc<Mutex<>>, Arc<RwLock<>>).
//!
//! ## Why These Handlers Are Here
//!
//! According to the Aura architecture, Layer 3 (aura-effects) MUST contain only stateless
//! effect handlers. However, many mock and testing handlers require state to provide
//! deterministic behavior, capture effects for verification, or simulate complex scenarios.
//!
//! ## Architecture Compliance
//!
//! By moving stateful handlers to Layer 8 (aura-testkit):
//! - Layer 3 maintains its stateless constraint
//! - Test infrastructure has access to stateful mocks when needed
//! - No circular dependencies are created (testkit is top-level)
//! - Production code remains clean of test artifacts
//!
//! ## Contents
//!
//! - Mock handlers with state: MockConsoleHandler, MockCryptoHandler, etc.
//! - Test infrastructure: MetricsHandler, MonitoringHandler, LoggingHandler
//! - Simulation handlers: MockSimulationHandler (use aura_effects::FallbackSimulationHandler for fallback)
//! - Memory transport handlers with shared registries
//! - Biometric mock handlers with template storage
//! - Time handlers with controllable time progression
//!
//! ## Usage in Tests
//!
//! ```rust,ignore
//! use aura_testkit::stateful_effects::{MockConsoleHandler, MockCryptoHandler};
//!
//! #[tokio::test]
//! async fn test_with_stateful_mocks() {
//!     let console = MockConsoleHandler::new();
//!     let crypto = MockCryptoHandler::with_seed(42);
//!
//!     // Use handlers for testing...
//!     console.log_info("Test message").await?;
//!     assert_eq!(console.captured_logs().len(), 1);
//! }
//! ```

pub mod authorization;
pub mod biometric;
pub mod console;
pub mod crypto;
pub mod journal;
pub mod leakage_handler;
pub mod random;
pub mod secure;
pub mod simulation;
pub mod storage;
pub mod system;
pub mod time;
pub mod transport;

// Re-export commonly used stateful handlers
pub use authorization::MockAuthorizationHandler;
pub use biometric::MockBiometricHandler;
pub use console::MockConsoleHandler;
pub use crypto::MockCryptoHandler;
pub use journal::MockJournalHandler;
// leakage_handler types removed - use aura_effects::ProductionLeakageHandler<S> with MemoryStorageHandler
pub use random::MockRandomHandler;
// secure::MockSecureStorageHandler removed - module is now a placeholder
pub use simulation::MockSimulationHandler;
pub use storage::MemoryStorageHandler;
pub use system::{LoggingHandler, MetricsHandler, MonitoringHandler};
pub use time::SimulatedTimeHandler;
pub use transport::InMemoryTransportHandler;
