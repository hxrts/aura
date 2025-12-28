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
    clippy::disallowed_methods, // Orchestration layer coordinates time/random effects
    deprecated // Deprecated time/random functions used intentionally for effect coordination
)]
//! Layer 4: Protocol-Specific Effect Traits
//!
//! This module provides protocol-level effect traits that extend the core effect system
//! from `aura-core` (Layer 1) with multi-party coordination capabilities.
//!
//! ## Import Guidelines
//!
//! - **Core effects** (`aura_core::effects`): Import directly for `CryptoEffects`,
//!   `NetworkEffects`, `StorageEffects`, `*TimeEffects`, `SessionType`, etc.
//! - **Protocol effects** (this module): Use for `ChoreographicEffects`, `EffectApiEffects`,
//!   `TreeEffects`, `SyncEffects`, and `AuraEffects` composite trait.
//!
//! ## Effect Hierarchy
//!
//! | Layer | Crate | Effects |
//! |-------|-------|---------|
//! | 1 | `aura-core` | Network, Crypto, Storage, Time, Random, Console, Session |
//! | 2 | Domain crates | Journal, Authorization, Capability |
//! | 4 | `aura-protocol` | Choreographic, EffectAPI, Tree, Semilattice, Sync |
//! | 7 | `aura-terminal` | TUI-specific effects |
//!
//! ## Protocol Effect Traits
//!
//! - [`ChoreographicEffects`]: Multi-party protocol coordination and projections
//! - [`EffectApiEffects`]: Event sourcing, audit trail, authorization tracking
//! - [`TreeEffects`]: Commitment tree operations and group key management
//! - [`SyncEffects`]: Anti-entropy, state reconciliation, gossip protocols
//! - Semilattice types: CRDT coordination with mathematical invariants
//!
//! ## Guard Chain
//!
//! Protocol messages expand through effects in order (per docs/109_authorization.md):
//! Authorization → FlowBudget → Leakage → Journal → Transport.
//!
//! ## Example
//!
//! ```rust,ignore
//! use aura_core::effects::{CryptoEffects, NetworkEffects, PhysicalTimeEffects};
//! use aura_protocol::effects::{ChoreographicEffects, AuraEffects};
//!
//! async fn execute_protocol_phase<E>(state: ProtocolState, effects: &E)
//!     -> Result<ProtocolState, ProtocolError>
//! where
//!     E: CryptoEffects + NetworkEffects + ChoreographicEffects,
//! {
//!     let signature = effects.ed25519_sign(&data, &key).await?;
//!     Ok(state.with_signature(signature))
//! }
//! ```

// Protocol-specific effect traits (Layer 4)
pub mod choreographic;
pub mod crdt;
pub mod effect_api;
pub mod params;
pub mod tree;

// Protocol effect re-exports
pub use choreographic::{
    ChoreographicEffects, ChoreographicRole, ChoreographyError, ChoreographyEvent,
    ChoreographyMetrics,
};

// Core effect re-exports (convenience - prefer importing from aura_core::effects directly)
// These are provided for backward compatibility and ergonomics in protocol code.
pub use aura_core::effects::{
    AuthorizationEffects, ConsoleEffects, CryptoEffects, CryptoError, JournalEffects,
    LeakageEffects, NetworkAddress, NetworkEffects, NetworkError, PeerEvent, PeerEventStream,
    RandomEffects, SecureStorageEffects, StorageEffects, StorageError, StorageLocation,
    StorageStats, SystemEffects, SystemError, ThresholdSigningEffects, TimeError, TimeoutHandle,
    WakeCondition,
};

// Time effect re-exports (unified time system)
pub use aura_core::effects::{LogicalClockEffects, OrderClockEffects, PhysicalTimeEffects};

// Crypto types (convenience re-export)
pub use aura_core::effects::crypto::{FrostSigningPackage, KeyDerivationContext};

pub use crdt::{
    ComposedHandler, CrdtCoordinator, CrdtCoordinatorError, DeliveryConfig, DeliveryEffect,
    DeliveryGuarantee, GossipStrategy, TopicId,
};
pub use effect_api::{EffectApiEffects, EffectApiError, EffectApiEvent, EffectApiEventStream};
pub use params::*; // Re-export all parameter types
                   // Sync effects re-exported from consolidated module
pub use crate::sync::effects::{AntiEntropyConfig, BloomDigest, SyncEffects, SyncError};
pub use tree::TreeEffects;

// Re-export unified error system
pub use aura_core::{AuraError, AuraResult};

// SystemEffects trait and SystemError now imported from aura-core
// (was previously in system_traits module, now moved to Layer 1)

// NOTE: Runtime infrastructure has been moved to aura-composition (Layer 3) and aura-agent (Layer 6)
// Handler composition available from aura_composition::
// - EffectBuilder, EffectRegistry, HandlerContainer
// Runtime assembly available from aura_agent::runtime:
// - AuraEffectSystem, LifecycleManager
// - Context management, services, optimizations
//
/// Composite trait that combines all effect traits
///
/// This trait combines all individual effect traits into a single trait object
/// that can be used by middleware and other components that need access to
/// multiple effect categories.
pub trait AuraEffects:
    CryptoEffects
    + NetworkEffects
    + StorageEffects
    + SecureStorageEffects
    + PhysicalTimeEffects
    + LogicalClockEffects
    + OrderClockEffects
    + RandomEffects
    + ConsoleEffects
    + JournalEffects
    + EffectApiEffects
    + TreeEffects
    + ChoreographicEffects
    + SystemEffects
    + AuthorizationEffects
    + LeakageEffects
    + ThresholdSigningEffects
    + Send
    + Sync
{
    /// Get the execution mode of this effects implementation
    fn execution_mode(&self) -> aura_core::effects::ExecutionMode;
}

// NOTE: Effect composition moved to aura-composition (Layer 3), runtime moved to aura-agent (Layer 6)
// Handler composition available from aura_composition::
// - EffectRegistry: Builder pattern for effect composition
// - EffectBuilder: Compile-time type-safe effect building
// Runtime assembly available from aura_agent::runtime:
// - AuraEffectSystem: Concrete effect system type

/// Protocol requirement specification marker
///
/// Used by protocol crates to surface the effect bounds they need at compile time.
/// Implementors should set `Requirements` to a concrete trait alias (e.g., `type Requirements = dyn AuraEffects;`)
/// so downstream code can enforce correct effect composition without runtime checks.
pub trait ProtocolRequirements {
    /// Type-level specification of required effects
    type Requirements;
}

// Note: AuraEffectSystem concrete type and implementations moved to aura-agent runtime
