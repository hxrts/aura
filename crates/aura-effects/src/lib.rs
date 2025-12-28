//! # Aura Effects - Layer 3: Implementation (Stateless Effect Handlers)
//!
//! **Purpose**: Production-grade stateless effect handlers that delegate to OS services.
//!
//! This crate provides the Implementation Layer of the Aura architecture,
//! containing context-free, single-operation effect handlers that work in ANY
//! execution context (production, testing, simulation, choreographic).
//!
//! # Architecture Constraints
//!
//! **Layer 3 depends only on aura-core and external libraries** (foundation + libraries).
//! - MUST implement infrastructure effect traits defined in aura-core
//! - MUST be stateless (no shared mutable state between calls)
//! - MUST be single-party (each handler works independently)
//! - MUST be context-free (no assumptions about caller's context)
//! - MUST NOT depend on other Aura crates (domain crates, aura-protocol, etc.)
//! - MUST NOT do multi-handler coordination
//! - MUST NOT do multi-party protocol logic
//!
//! ## Stateful Boundary (compile-fail example)
//!
//! Handlers in Layer 3 are stateless. Stateful caches belong in Layer 6 services.
//!
//! ```compile_fail
//! use std::sync::RwLock;
//!
//! struct BadHandler {
//!     cache: RwLock<Vec<u8>>,
//! }
//!
//! compile_error!("Stateful caches must live in Layer 6 handlers, not aura-effects.");
//! ```
//!
//! # Required Infrastructure Effects
//!
//! This crate MUST provide handlers for:
//! - CryptoEffects: Ed25519 signing, hashing, key derivation
//! - NetworkEffects: TCP connections, message sending
//! - StorageEffects: File I/O, chunk operations
//! - TimeEffects: Current time, delays
//! - RandomEffects: Cryptographically secure randomness
//!
//! # What Belongs Here
//!
//! Basic effect implementations (RealCryptoHandler, ProductionLeakageHandler)
//! Storage backends (FilesystemStorageHandler, EncryptedStorage)
//! Network transports (TcpTransportHandler, WebSocketTransportHandler)
//! Time providers (RealTimeHandler), System handlers (LoggingSystemHandler)
//!
//! # What Does NOT Belong Here
//!
//! ❌ Multi-handler coordination (→ aura-protocol)
//! ❌ Choreographic bridges (→ aura-protocol)
//! ❌ Stateful orchestration (→ aura-protocol)
//! ❌ Complete protocols (→ feature crates)
//!
//! # Usage
//!
//! ```rust,ignore
//! use aura_effects::crypto::RealCryptoHandler;
//! use aura_effects::storage::FilesystemStorageHandler;
//! use aura_core::effects::{CryptoEffects, StorageEffects};
//!
//! // Use handlers directly for single operations
//! let crypto = RealCryptoHandler::new();
//! let signature = crypto.sign(&key, &message).await?;
//!
//! // Or compose into a runtime (done by aura-agent or aura-protocol)
//! let runtime = RuntimeBuilder::new()
//!     .with_crypto(crypto)
//!     .with_storage(storage)
//!     .build();
//! ```

// NOTE: AuthorizationEffects moved to aura-authorization (domain crate) per Layer 2 pattern
pub mod biometric;
pub mod console;
pub mod context;
/// Cryptographic effect handlers for signing, verification, and key derivation
pub mod crypto;
/// Indexed journal handler with B-tree indexes, Bloom filters, and Merkle trees
pub mod database;
// NOTE: JournalEffects moved to aura-journal (domain crate) per Layer 2 pattern
/// Unified encrypted storage wrapper for transparent encryption at rest
pub mod encrypted_storage;
/// Error types for Layer 3 handler implementations.
pub mod error;
pub mod guard_interpreter;
pub mod leakage;
pub mod query;
pub mod random;
pub mod reactive;
pub mod secure;
#[cfg(feature = "simulation")]
pub mod simulation;
pub mod storage;
// sync_bridge removed - replaced by pure guard evaluation (ADR-014)
pub mod system;
pub mod time;
pub mod trace;
pub mod transport;

// Re-export production handlers only - mock handlers moved to aura-testkit
// NOTE: WotAuthorizationHandler moved to aura-authorization per Layer 2 pattern
pub use biometric::FallbackBiometricHandler;
pub use console::RealConsoleHandler;
pub use context::{ExecutionContext, StandardContextHandler};
pub use crypto::RealCryptoHandler;
pub use database::query::{AuraQuery, FactTerm, QueryError, QueryResult};
pub use error::Layer3Error;
pub use query::{
    format_rule, format_value, parse_arg_to_value, parse_fact_to_row, CapabilityPolicy,
    QueryHandler,
};
// NOTE: JournalHandler moved to aura-journal per Layer 2 pattern
pub use guard_interpreter::ProductionEffectInterpreter;
pub use leakage::ProductionLeakageHandler;
pub use random::RealRandomHandler;
pub use reactive::{ReactiveHandler, SignalGraph, SignalGraphStats};
pub use secure::RealSecureStorageHandler;
#[cfg(feature = "simulation")]
pub use simulation::FallbackSimulationHandler;
pub use storage::FilesystemStorageHandler;
// Re-export the new unified encrypted storage (Task 1.1)
pub use encrypted_storage::{EncryptedStorage, EncryptedStorageConfig};
// ProductionSyncExecutor removed - replaced by ProductionEffectInterpreter (ADR-014)
#[allow(deprecated)]
pub use time::{
    LogicalClockHandler, OrderClockHandler, PhysicalTimeHandler, TimeComparisonHandler,
};

// Note: AuthorizationEffects + JournalEffects are provided by layer 2 domain crates

// Transport effect handlers - organized by functionality
pub mod transport_effects {
    //! Transport effect implementations - Layer 3 stateless handlers

    pub use crate::transport::{
        // Utilities and helpers
        AddressResolver,
        BufferUtils,
        ConnectionMetrics,
        // Message processing
        FramingHandler,

        // InMemoryTransportHandler moved to aura-testkit

        // Facade patterns removed - migrate to aura-protocol
        // RetryingTransportManager,
        // TransportManager,

        // Core transport handlers
        RealTransportHandler,
        TcpTransportHandler,
        TimeoutHelper,
        // Integration helpers
        TransportError,
        UrlValidator,

        WebSocketTransportHandler,
    };
}

// Convenience re-exports for most common handlers
// Re-export system handlers
pub use system::{LoggingSystemHandler, MetricsSystemHandler, MonitoringSystemHandler};

// Convenience re-exports for most common transport handlers
pub use transport_effects::{
    FramingHandler, RealTransportHandler, TcpTransportHandler, TransportError,
    WebSocketTransportHandler,
};

// Re-export core effect traits for convenience
pub use aura_core::effects::*;

// Compatibility bridge has been removed after fixing all architectural violations
