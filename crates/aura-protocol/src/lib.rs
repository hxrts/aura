#![allow(missing_docs)]
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
//! // Create unified effect system for testing
//! let handler = AuraEffectSystem::for_testing(device_id);
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
pub mod effects;
pub mod guards;
pub mod handlers;
pub mod messages;
pub mod middleware;
pub mod sync;
pub mod verification;

// Unified AuraEffectSystem architecture only

// Public API re-exports
pub use effects::{
    AntiEntropyConfig, AuraEffectSystem, AuraEffectSystemFactory, AuraEffectSystemStats,
    AuraEffects, BloomDigest, ChoreographicEffects, ChoreographicRole, ChoreographyEvent,
    ChoreographyMetrics, ConsoleEffects, ConsoleEvent, CryptoEffects, DeviceMetadata,
    JournalEffects, LedgerEffects, LedgerError, LedgerEvent, LedgerEventStream, LogLevel,
    NetworkAddress, NetworkEffects, NetworkError, RandomEffects, StorageEffects, StorageError,
    StorageLocation, SyncEffects, SyncError, TimeEffects, WakeCondition,
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

pub use guards::{
    apply_delta_facts, evaluate_guard, execute_guarded_operation, track_leakage_consumption,
    ExecutionMetrics, GuardedExecutionResult, LeakageBudget, ProtocolGuard,
};

pub use sync::{IntentState, PeerView};

pub use authorization_bridge::{
    authenticate_and_authorize, evaluate_authorization, AuthorizationContext, AuthorizationError,
    AuthorizationRequest, AuthorizedEvent, PermissionGrant,
};

pub use verification::{
    CapabilitySoundnessVerifier, CapabilityState, SoundnessProperty, SoundnessReport,
    SoundnessVerificationResult, VerificationConfig,
};

// Clean API - no legacy compatibility

// Convenient prelude for common imports
pub mod prelude {
    //! Prelude for common imports
    //!
    //! This module re-exports the most commonly used types and traits for convenient importing.

    // Core effect traits
    pub use crate::effects::{
        ChoreographicEffects, ConsoleEffects, CryptoEffects, JournalEffects, LedgerEffects,
        NetworkEffects, RandomEffects, StorageEffects, TimeEffects,
    };

    // Common error types
    pub use crate::effects::{ChoreographyError, LedgerError, NetworkError, StorageError};

    // Utility types
    pub use crate::effects::{
        ChoreographicRole, ChoreographyEvent, ChoreographyMetrics, ConsoleEvent, NetworkAddress,
        StorageLocation, WakeCondition,
    };

    // Handler types
    pub use crate::handlers::{
        AuraHandler, AuraHandlerFactory, CompositeHandler, EffectType, ExecutionMode,
    };

    // Authorization bridge
    pub use crate::authorization_bridge::{
        authenticate_and_authorize, evaluate_authorization, AuthorizationContext,
        AuthorizationRequest, AuthorizedEvent, PermissionGrant,
    };

    // Middleware types
    pub use crate::middleware::{
        HandlerMetadata, MiddlewareContext, MiddlewareError, MiddlewareResult, PerformanceProfile,
    };

    // Context types
    pub use crate::handlers::AuraContext;

    // External dependencies commonly used with this crate
    pub use async_trait::async_trait;
    pub use aura_core::{AuraError, AuraResult, DeviceId};
    pub use uuid::Uuid;

    // Common standard library types
    pub use std::collections::HashMap;
    pub use std::sync::Arc;
    pub use std::time::Duration;
}

// Version information
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
