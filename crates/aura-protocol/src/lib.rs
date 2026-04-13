#![deny(clippy::await_holding_lock)]
#![deny(clippy::disallowed_types)]
#![deny(clippy::dbg_macro)]
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
//!
//! ## Example
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

#[cfg(all(feature = "transparent_onion", not(any(test, debug_assertions))))]
compile_error!(
    "Feature `transparent_onion` is a debug/test/simulation-only tool and must \
     not be enabled in release production builds."
);

// Core modules following unified effect system architecture
pub use aura_amp as amp;
pub use aura_anti_entropy as sync;
pub mod admission;
pub mod choreography;
pub mod config;
pub mod effects;
pub mod error;
pub mod facades;
pub mod handlers;
pub mod messages;
pub mod prelude;
pub mod session;
pub mod state;
pub mod termination;
pub mod types;

pub use error::ProtocolError;

// Re-export session types for convenient access
pub use session::{SessionOutcome, SessionStatus};
pub use termination::{
    TerminationBudget, TerminationBudgetConfig, TerminationBudgetError, TerminationProtocolClass,
};

// Re-export protocol orchestration types for convenient access
pub use types::{
    ProtocolDuration, ProtocolMode, ProtocolPriority, ProtocolSessionStatus, ProtocolType,
};

/// High-level protocol coordination and execution
///
/// Use this module when implementing distributed protocols that need:
/// - Protocol orchestration and choreography
/// - Anti-entropy coordination
/// - Standard pattern abstractions (facades)
/// - Device metadata management
/// - Protocol messaging and guards
pub mod orchestration {
    pub use crate::effects::AuraEffects;
    pub use crate::effects::{AntiEntropyConfig, BloomDigest};
    pub use crate::effects::{
        ChoreographicEffects, ChoreographicRole, ChoreographyEvent, ChoreographyMetrics,
    };
    pub use crate::facades::{ProtocolOrchestrator, StandardPatterns};
    pub use crate::handlers::{AuraContext, ExecutionMode};
    pub use crate::messages::{AuraMessage, CryptoMessage, CryptoPayload, WIRE_FORMAT_VERSION};
}

/// Standard effect composition patterns and bundles
///
/// Use this module when setting up effect systems using proven patterns:
/// - Pre-configured effect bundles (testing, production, simulation)
/// - Standard registry patterns
/// - Protocol requirements declaration
pub mod standard_patterns {
    pub use crate::effects::ProtocolRequirements;
}

/// Effect system assembly and configuration tools
///
/// Use this module when building custom effect systems:
/// - Type-safe effect composition
/// - Handler management and factories
/// - Custom effect system assembly
pub mod composition {
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

// Version information
/// Current crate version for protocol orchestration.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
