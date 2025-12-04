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
    clippy::single_range_in_vec_init,
    clippy::disallowed_methods,  // Orchestration layer coordinates time/random effects
    clippy::disallowed_types,
    deprecated  // Deprecated time/random functions used intentionally for effect coordination
)]
//! # Aura Protocol - Layer 4: Orchestration (Multi-Party Coordination)
//!
//! **Purpose**: Multi-party coordination and distributed protocol orchestration.
//!
//! This crate provides coordination of effects and multi-party orchestration for Aura's
//! distributed protocols. It is the "choreography conductor" that ensures complex
//! multi-party coordination executes correctly across network boundaries.
//!
//! # Architecture Constraints
//!
//! **Layer 4 depends on aura-core, aura-effects, aura-composition, aura-mpst, and domain crates**.
//! - MUST coordinate multiple handlers working together
//! - MUST implement guard chain (CapGuard -> FlowGuard -> JournalCoupler)
//! - MUST provide multi-party protocol orchestration
//! - MUST integrate Aura Consensus and anti-entropy
//! - MUST NOT implement individual effect handlers (that's aura-effects)
//! - MUST NOT implement handler composition (that's aura-composition)
//! - MUST NOT implement application-specific protocol logic (that's Layer 5 feature crates)
//!
//! # Key Components
//!
//! - Guard chain coordination (CapGuard -> FlowGuard -> JournalCoupler)
//! - Multi-party protocol orchestration and state management
//! - CRDT coordinator for handling consensus and anti-entropy
//! - Cross-handler coordination logic
//! - Aura Consensus integration for strong agreement
//!
//! # What Belongs Here
//!
//! Multi-party coordination across network boundaries:
//! - Guard chain enforcement (authorization, flow budgets, journal coupling)
//! - Protocol orchestration patterns
//! - Handler coordination and communication
//! - Distributed consensus and anti-entropy
//!
//! # What Does NOT Belong Here
//!
//! - Individual effect handler implementations (that's aura-effects)
//! - Handler composition infrastructure (that's aura-composition)
//! - Single-party effect operations (that's aura-effects)
//! - Application-specific protocol details (that's Layer 5 feature crates)
//! - Runtime composition and lifecycle (that's aura-agent)
//! ### Effects (`effects/`)
//! Pure trait definitions for all side-effect operations:
//! - **NetworkEffects**: Peer communication, message passing
//! - **StorageEffects**: Data persistence, key-value operations
//! - **CryptoEffects**: Cryptographic operations, secure randomness
//! - **PhysicalTimeEffects**: Physical clock access, timestamp generation
//! - **LogicalClockEffects**: Causal ordering, vector clocks
//! - **OrderClockEffects**: Privacy-preserving deterministic ordering
//! - **ConsoleEffects**: Logging, debugging, visualization
//! - **EffectApiEffects**: Account state, event sourcing
//! - **ChoreographicEffects**: Distributed protocol coordination
//!
//! ### Handlers (`handlers/`)
//! Concrete implementations of effect traits for different contexts:
//! - **Multiple implementations per effect** (real, mock, simulation)
//! - **AuraHandler**: Type-erased handler traits for dynamic dispatch
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
//!     E: NetworkEffects + CryptoEffects + PhysicalTimeEffects,
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
pub mod amp;
pub mod authorization; // Biscuit-based authorization (moved up from authorization/ subdirectory)
pub mod choreography;
pub mod consensus; // Real Aura Consensus implementation
pub mod context;
pub mod effects;
pub mod error;
pub mod facades; // High-level facade traits (Layer 4 appropriate - traits only, implementations in Layer 6)
pub mod guards;
pub mod handlers;
pub mod messages;
pub mod moderation; // Query/reduction layer for moderation state from journal facts
pub mod session; // Session orchestration types (SessionStatus, SessionOutcome)
pub mod state;
pub mod transport;
pub mod types; // Protocol orchestration types

// Re-export authorization types for convenient access
pub use authorization::{AuthorizationResult, BiscuitAuthorizationBridge};
pub use error::ProtocolError;

// Re-export session types for convenient access
pub use session::{SessionOutcome, SessionStatus};

// Re-export protocol orchestration types for convenient access
pub use types::{
    ProtocolDuration, ProtocolMode, ProtocolPriority, ProtocolSessionStatus, ProtocolType,
};

// Unified AuraEffectSystem architecture only

// ============================================================================
// PHASE 2.2: CAPABILITY INTERFACE GROUPING
// ============================================================================
//
// Organized exports into focused capability groups for better discoverability
// and cleaner public API. Each group serves a specific use case.

/// High-level protocol coordination and execution
///
/// Use this module when implementing distributed protocols that need:
/// - Protocol orchestration and choreography
/// - Anti-entropy coordination
/// - Standard pattern abstractions (facades)
/// - Device metadata management
/// - Protocol messaging and guards
pub mod orchestration {
    // High-level facade traits (Layer 4 appropriate)
    pub use crate::facades::{ProtocolOrchestrator, StandardPatterns};

    // Core system (AuraEffectSystem moved to aura-agent runtime)
    pub use crate::effects::AuraEffects;

    // Protocol coordination
    pub use crate::effects::{
        ChoreographicEffects, ChoreographicRole, ChoreographyEvent, ChoreographyMetrics,
    };

    // Configuration and coordination
    pub use crate::effects::{AntiEntropyConfig, BloomDigest};

    // Context and execution
    pub use crate::handlers::{AuraContext, ExecutionMode};

    // Protocol messaging
    pub use crate::messages::{AuraMessage, CryptoMessage, CryptoPayload, WIRE_FORMAT_VERSION};

    // Security and budgets
    pub use crate::guards::{LeakageBudget, ProtocolGuard};
}

/// Standard effect composition patterns and bundles
///
/// Use this module when setting up effect systems using proven patterns:
/// - Pre-configured effect bundles (testing, production, simulation)
/// - Standard registry patterns
/// - Protocol requirements declaration
pub mod standard_patterns {
    // NOTE: Standard bundles moved to aura-composition for Layer 3 handler composition
    // Use aura_composition::{EffectRegistry, EffectBuilder} for effect composition

    // Pattern building (EffectBundle moved to aura-composition)
    pub use crate::effects::ProtocolRequirements;
}

/// Effect system assembly and configuration tools
///
/// Use this module when building custom effect systems:
/// - Type-safe effect composition
/// - Handler management and factories
/// - Custom effect system assembly
pub mod composition {
    // High-level facade
    // NOTE: Concrete facades moved to aura-agent for runtime assembly
    // pub use crate::facades::EffectComposer;

    // NOTE: Builder pattern moved to aura-composition for Layer 3 handler composition
    // Use aura_composition::{EffectBuilder, EffectRegistryError} for effect building

    // Handler management
    pub use crate::handlers::core::erased::AuraHandlerFactory;
    pub use crate::handlers::{AuraHandler, EffectType, HandlerUtils};
}

/// Individual effect trait definitions
///
/// Use this module when implementing protocols that need specific effects:
/// - Core effect traits (Crypto, Network, Storage, etc.)
/// - Associated types and error handling
/// - Fine-grained effect selection
pub mod effect_traits {
    // Core traits
    pub use crate::effects::{
        ConsoleEffects, CryptoEffects, EffectApiEffects, JournalEffects, LogicalClockEffects,
        NetworkEffects, OrderClockEffects, PhysicalTimeEffects, RandomEffects, StorageEffects,
        SyncEffects,
    };

    // Associated types and errors
    pub use crate::effects::{
        EffectApiError, EffectApiEvent, EffectApiEventStream, NetworkAddress, NetworkError,
        StorageError, StorageLocation, SyncError, WakeCondition,
    };
}

/// Internal implementation details
///
/// These exports are for internal use and may be removed in future versions.
/// Prefer using the capability interfaces above.
pub mod internal {
    // Error handling for handlers
    pub use crate::handlers::AuraHandlerError;

    // Version metadata
    pub use crate::VERSION;
}

// Core effect trait - for protocol interfaces
pub use effects::AuraEffects;

// Note: AuraEffectSystem, EffectRegistry, and effect bundles are in aura-agent runtime
// aura-protocol (Layer 4) should not depend on aura-agent (Layer 6)
// See: docs/001_system_architecture.md for correct layering
//
// Legacy re-exports removed (v0.2.0). Use qualified paths from appropriate modules:
// - orchestration::* for coordination types
// - effect_traits::* for effect trait definitions
// - composition::* for handler composition

// IntentState and PeerView removed - only used in internal tests

// Only export authorization types actually used by other crates
// pub use authorization_bridge::{ // Module removed - replaced by authorization module
//     AuthorizationContext, AuthorizationError, AuthorizationMetadata, AuthorizationRequest,
//     AuthorizationService, AuthorizedEvent, PermissionGrant,
// };

// Verification module removed from public API - test-only code
// (verification module still exists for internal tests)

// Transport coordination removed from public API - never used by dependent crates
// Decision needed: evaluate if transport/ should move to aura-transport crate

// Clean API - no legacy compatibility

// Prelude module removed - zero usage across workspace

// Version information
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
