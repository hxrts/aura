#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::type_complexity,
    clippy::while_let_loop
)]

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
//! Storage backends (FilesystemStorageHandler, EncryptedStorageHandler)
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

pub mod authorization;
pub mod biometric;
pub mod bloom;
pub mod console;
pub mod context;
/// Cryptographic effect handlers for signing, verification, and key derivation
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

// Re-export production handlers only - mock handlers moved to aura-testkit
pub use authorization::StandardAuthorizationHandler;
pub use biometric::RealBiometricHandler;
pub use bloom::BloomHandler;
pub use console::RealConsoleHandler;
pub use context::{ExecutionContext, StandardContextHandler};
pub use crypto::{EffectSystemRng, RealCryptoHandler};
pub use journal::StandardJournalHandler;
pub use leakage_handler::ProductionLeakageHandler;
pub use random::RealRandomHandler;
pub use secure::RealSecureStorageHandler;
pub use simulation::StatelessSimulationHandler;
pub use storage::{EncryptedStorageHandler, FilesystemStorageHandler};
pub use time::RealTimeHandler;
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
    FramingHandler, RealTransportHandler, TcpTransportHandler, 
    TransportError, WebSocketTransportHandler,
};

// Re-export core effect traits for convenience
pub use aura_core::effects::*;

// Compatibility bridge has been removed after fixing all architectural violations
