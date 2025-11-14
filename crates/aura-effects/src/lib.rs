#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::type_complexity,
    clippy::while_let_loop
)]

//! Aura Effects - Standard Effect Handler Implementations
//!
//! This crate provides the **Implementation Layer** of the Aura architecture,
//! containing context-free, single-operation effect handlers that work in ANY
//! execution context (production, testing, simulation, choreographic).
//!
//! # Architecture Position
//!
//! ```text
//! User Interface Layer (aura-cli, app-*)
//!     ↓
//! Runtime Composition (aura-agent, simulator)
//!     ↓
//! Feature/Protocol (frost, invitation, etc.)
//!     ↓
//! Orchestration Layer (aura-protocol)
//!     ↓
//! Implementation Layer (aura-effects) ← YOU ARE HERE
//!     ↓
//! Specification Layer (aura-mpst + domains)
//!     ↓
//! Interface Layer (aura-core)
//! ```
//!
//! # Key Characteristics
//!
//! All handlers in this crate are:
//! - **Stateless**: No coordination state between operations
//! - **Single-party**: Work for one device in isolation
//! - **Context-free**: No assumptions about execution context
//! - **Single-operation**: Implement individual effect trait methods
//!
//! # What Belongs Here
//!
//! Basic effect implementations (RealCryptoHandler, MockCryptoHandler)
//! Storage backends (FilesystemStorageHandler, InMemoryStorageHandler)
//! Network transports (TcpNetworkHandler, MockNetworkHandler)
//! Time providers (RealTimeHandler, SimulatedTimeHandler)
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
//! use aura_effects::storage::InMemoryStorageHandler;
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

pub mod console;
pub mod crypto;
pub mod journal;
pub mod random;
pub mod storage;
pub mod time;
pub mod transport;

// Re-export commonly used handlers
pub use console::{MockConsoleHandler, RealConsoleHandler};
pub use crypto::{MockCryptoHandler, RealCryptoHandler};
pub use journal::MemoryJournalHandler;
pub use random::{MockRandomHandler, RealRandomHandler};
pub use storage::{FilesystemStorageHandler, MemoryStorageHandler};
pub use time::{RealTimeHandler, SimulatedTimeHandler};
// Transport effect handlers - organized by functionality
pub mod transport_effects {
    //! Transport effect implementations - Layer 3 stateless handlers
    
    pub use crate::transport::{
        // Core transport handlers  
        TcpTransportHandler,
        WebSocketTransportHandler, 
        InMemoryTransportHandler,
        
        // Message processing
        FramingHandler,
        
        // Utilities and helpers
        AddressResolver,
        TimeoutHelper,
        BufferUtils,
        ConnectionMetrics,
        UrlValidator,
        
        // Coordination helpers (NO choreography)
        TransportManager,
        RetryingTransportManager,
        
        // Integration helpers
        TransportError,
    };
}

// Convenience re-exports for most common handlers
pub use transport_effects::{
    TcpTransportHandler, WebSocketTransportHandler, InMemoryTransportHandler,
    FramingHandler, TransportManager, TransportError
};

// Re-export core effect traits for convenience
pub use aura_core::effects::*;

// Compatibility bridge has been removed after fixing all architectural violations
