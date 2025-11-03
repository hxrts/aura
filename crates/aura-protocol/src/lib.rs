//! Aura Protocol - Algebraic Effects Architecture
//!
//! This crate provides a clean algebraic effects architecture for Aura's distributed protocols.
//! Following the algebraic effects pattern, it separates effect definitions (what can be done)
//! from effect handlers (how effects are implemented) and middleware (cross-cutting concerns).
//!
//! ## Quick Start
//!
//! ```rust,ignore
//! use aura_protocol::prelude::*;
//! use uuid::Uuid;
//!
//! // Create an execution context for testing
//! let context = ContextBuilder::new()
//!     .with_device_id(DeviceId::from(Uuid::new_v4()))
//!     .with_participants(vec![device_id])
//!     .build_for_testing();
//!
//! // Use effects in your protocol
//! let random_bytes = context.effects.random_bytes(32).await;
//! context.effects.send_to_peer(peer_id, message).await?;
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
//! - **Caching**: Result caching and memoization
//!
//! ### Runtime (`runtime/`)
//! Execution context and session management:
//! - **ExecutionContext**: Environment for protocol execution
//! - **SessionManager**: Protocol session lifecycle
//! - **EffectExecutor**: Coordinates effect operations
//!
//! ## Examples
//!
//! ### Basic Usage
//! ```rust,ignore
//! use aura_protocol::prelude::*;
//!
//! // Create handler with middleware
//! let handler = CompositeHandler::for_production(device_id);
//! let enhanced = MiddlewareStack::new(handler, device_id)
//!     .with_tracing("my-service".to_string())
//!     .with_metrics()
//!     .with_retry(RetryConfig::default())
//!     .build();
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

// Core modules following algebraic effects pattern
pub mod effects;
pub mod handlers;
pub mod middleware;
pub mod runtime;

// Clean algebraic effects architecture only

// Public API re-exports
pub use effects::{
    // Effect traits
    NetworkEffects, StorageEffects, CryptoEffects, TimeEffects,
    ConsoleEffects, LedgerEffects, ChoreographicEffects,
    ProtocolEffects, MinimalEffects,
    
    // Error types
    NetworkError, StorageError, CryptoError, TimeError,
    
    // Common types
    WakeCondition, TimeoutHandle, PeerEvent, StorageStats,
    ChoreographicRole, ChoreographyEvent,
};

pub use handlers::{
    CompositeHandler, HandlerBuilder,
    
    // Individual handlers for advanced use
    MemoryNetworkHandler, RealNetworkHandler, SimulatedNetworkHandler,
    MockCryptoHandler, RealCryptoHandler,
    RealTimeHandler, SimulatedTimeHandler,
    MemoryStorageHandler, FilesystemStorageHandler,
    SilentConsoleHandler, StdoutConsoleHandler, StructuredConsoleHandler,
};

pub use middleware::{
    // Middleware types
    MiddlewareStack, MiddlewareConfig, Middleware,
    
    // Specific middleware
    TracingMiddleware, MetricsMiddleware,
    RetryMiddleware, RetryConfig,
    CapabilityMiddleware, AuthorizationMiddleware,
    
    // Middleware utilities
    create_standard_stack,
};

pub use runtime::{
    ExecutionContext, ContextBuilder,
    SessionManager, SessionState, SessionStatus, SessionConfig,
    EffectExecutor, ExecutorConfig, ExecutionMode,
};

// Clean API - no legacy compatibility

// Convenient prelude for common imports
pub mod prelude;

// Version information
pub const VERSION: &str = env!("CARGO_PKG_VERSION");