#![allow(
    missing_docs,
    unused_variables,
    clippy::unwrap_used,
    clippy::expect_used,
    dead_code,
    clippy::match_like_matches_macro,
    clippy::type_complexity,
    clippy::while_let_loop,
    clippy::redundant_closure,
    clippy::large_enum_variant,
    clippy::unused_unit,
    clippy::get_first,
    clippy::single_range_in_vec_init
)]
//! Aura Protocol - Algebraic Effects Architecture
//!
//! This crate provides a clean algebraic effects architecture for Aura's distributed protocols.
//! Following the algebraic effects pattern, it separates effect definitions (what can be done)
//! from effect handlers (how effects are implemented) and middleware (cross-cutting concerns).
//!
//! ## Quick Start
//!
//! ```rust,ignore
//! use crate::prelude::*;
//! use uuid::Uuid;
//!
//! // Create unified effect system for testing using modern testkit pattern  
//! let fixture = aura_testkit::create_test_fixture_with_device_id(device_id).await?;
//! let handler = fixture.effect_system();
//!
//! // Use effects directly with zero overhead
//! let random_bytes = handler.random_bytes(32).await;
//! handler.send_to_peer(peer_id, message).await?;
//! ```
//!
//! ## Architecture Overview
//!
//! ### Effects (`effects/`)
//! Pure trait definitions for all side-effect operations:
//! - **NetworkEffects**: Peer communication, message passing
//! - **StorageEffects**: Data persistence, key-value operations
//! - **CryptoEffects**: Cryptographic operations, secure randomness
//! - **TimeEffects**: Scheduling, timeouts, temporal coordination
//! - **ConsoleEffects**: Logging, debugging, visualization
//! - **LedgerEffects**: Account state, event sourcing
//! - **ChoreographicEffects**: Distributed protocol coordination
//!
//! ### Handlers (`handlers/`)
//! Concrete implementations of effect traits for different contexts:
//! - **Multiple implementations per effect** (real, mock, simulation)
//! - **CompositeHandler**: Unified handler implementing all effects
//! - **Context-aware selection**: Testing vs Production vs Simulation
//!
//! ### Middleware (`middleware/`)
//! Effect decorators that add cross-cutting concerns:
//! - **Observability**: Tracing, metrics, logging
//! - **Resilience**: Retry, timeout, circuit breaker
//! - **Security**: Authorization, capability checking
//! - **Authorization Bridge**: Connect authentication with authorization
//! - **Caching**: Result caching and memoization
//!
//! ## Examples
//!
//! ### Basic Usage
//! ```rust,ignore
//! use crate::prelude::*;
//!
//! // Create unified system with optional middleware
//! let base = AuraEffectSystem::for_production(device_id)?;
//! let enhanced = TracingMiddleware::new(
//!     MetricsMiddleware::new(
//!         RetryMiddleware::new(base, 3)
//!     ),
//!     "my-service"
//! );
//! ```
//!
//! ### Protocol Implementation
//! ```rust,ignore
//! async fn my_protocol<E>(effects: &E) -> Result<Vec<u8>, ProtocolError>
//! where
//!     E: NetworkEffects + CryptoEffects + TimeEffects,
//! {
//!     // Generate random nonce
//!     let nonce = effects.random_bytes(32).await;
//!
//!     // Send to peer
//!     effects.send_to_peer(peer_id, nonce.clone()).await?;
//!
//!     // Wait for response
//!     let (from, response) = effects.receive().await?;
//!
//!     Ok(response)
//! }
//! ```

// Core modules following unified effect system architecture
pub mod authorization_bridge;
pub mod choreography;
pub mod context;
pub mod effects;
pub mod guards;
pub mod handlers;
pub mod messages;
pub mod state;
pub mod transport;

// Unified AuraEffectSystem architecture only

// Public API re-exports
pub use effects::{
    AntiEntropyConfig,
    AuraEffectSystem, // AuraEffectSystemFactory and AuraEffectSystemStats removed (legacy system)
    AuraEffects,
    BloomDigest,
    ChoreographicEffects,
    ChoreographicRole,
    ChoreographyEvent,
    ChoreographyMetrics,
    ConsoleEffects,
    CryptoEffects,
    DeviceMetadata,
    JournalEffects,
    LedgerEffects,
    LedgerError,
    LedgerEvent,
    LedgerEventStream,
    NetworkAddress,
    NetworkEffects,
    NetworkError,
    RandomEffects,
    StorageEffects,
    StorageError,
    StorageLocation,
    SyncEffects,
    SyncError,
    TimeEffects,
    WakeCondition,
};

pub use handlers::{
    AuraContext,
    AuraHandler,
    AuraHandlerError,

    // Factory and utilities
    AuraHandlerFactory,
    EffectType,
    ExecutionMode,
    HandlerUtils,
};

pub use messages::{AuraMessage, CryptoMessage, CryptoPayload, WIRE_FORMAT_VERSION};

// Only export guards actually used by other crates
pub use guards::{LeakageBudget, ProtocolGuard};

// IntentState and PeerView removed - only used in internal tests

// Only export authorization types actually used by other crates
pub use authorization_bridge::{AuthorizationContext, AuthorizationError};

// Verification module removed from public API - test-only code
// (verification module still exists for internal tests)

// Transport coordination removed from public API - never used by dependent crates
// Decision needed: evaluate if transport/ should move to aura-transport crate

// Clean API - no legacy compatibility

// Prelude module removed - zero usage across workspace

// Version information
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
