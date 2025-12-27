//! # Aura Sync - Layer 5: Feature/Protocol Implementation
//!
//! This crate provides complete end-to-end synchronization protocol implementations.
//!
//! ## Purpose
//!
//! Layer 5 feature crate providing reusable protocol building blocks for:
//! - Journal state synchronization using CRDT semilattice semantics
//! - Anti-entropy protocols for state reconciliation between peers
//! - Snapshot creation and restoration for efficient sync initialization
//! - OTA upgrade coordination with threshold approval
//! - Receipt verification for distributed protocol commitment
//!
//! ## Architecture Constraints
//!
//! This crate depends on:
//! - **Layer 1** (aura-core): Core types, effects, errors, session management
//! - **Layer 2** (aura-journal): CRDT semantics and fact storage
//! - **Layer 3** (aura-effects): Effect handler implementations
//! - **Layer 4** (aura-protocol): Orchestration and guard chain
//!
//! ## What Belongs Here
//!
//! - Complete protocol implementations (anti-entropy, journal sync, snapshots, OTA, receipts)
//! - Protocol coordination for multi-party synchronization
//! - Configuration and policy for sync strategies
//! - Metrics collection and health monitoring
//! - Session management for protocol coordination
//! - Infrastructure utilities (peer management, retry logic, caching, rate limiting)
//! - MPST protocol definitions for sync ceremonies
//!
//! ## What Does NOT Belong Here
//!
//! - Effect handler implementations (belong in aura-effects)
//! - Handler composition or registry (belong in aura-composition)
//! - Low-level multi-party coordination (belong in aura-protocol)
//! - Journal implementations (belong in aura-journal)
//! - Storage backend implementations
//!
//! ## Design Principles
//!
//! - All protocols parameterized by effect traits: generic over effect implementations
//! - Protocols are effect-driven: composition, testing, and deployment flexibility
//! - CRDT semantics ensure idempotent, convergent synchronization
//! - Anti-entropy ensures eventual consistency without coordination overhead
//! - Session-based coordination allows stateless protocol implementation
//! - Integration with guard chain ensures authorization before synchronization
//! - Metrics collection enables observability and performance tuning
//!
//! ## Authority vs Device Model
//!
//! This crate uses Aura's authority-centric identity model:
//!
//! - **`AuthorityId`**: Represents the *owner* of state and operations. Authorities are
//!   cryptographic identities that own journals, create attestations, and authorize actions.
//!   State is synchronized *per authority*, not per device.
//!
//! - **`DeviceId`**: Represents a *connection endpoint* for network communication.
//!   Devices connect to each other to exchange state, but state ownership is always
//!   attributed to authorities, not devices.
//!
//! In sync protocols:
//! - Peers are identified by `DeviceId` for network addressing
//! - Journal operations and facts are attributed to `AuthorityId`
//! - Authorization decisions use `AuthorityId` via Biscuit tokens
//! - State merges resolve conflicts using authority-attributed timestamps
//!
//! See `docs/100_authority_and_identity.md` for the complete authority model.
//!
//! ## Time System
//!
//! This crate uses the unified time system from `aura-core`:
//!
//! - **`PhysicalTime`**: Wall-clock timestamps with optional uncertainty bounds.
//!   Used for timestamps, timeouts, and coordination deadlines.
//! - All time access goes through `PhysicalTimeEffects` trait, never direct `SystemTime` calls
//! - Time is passed explicitly to methods, enabling deterministic testing
//!
//! See `docs/106_effect_system_and_runtime.md` for the unified time architecture.

// Allow disallowed methods/types in protocol implementations that coordinate effects
#![allow(clippy::disallowed_methods, clippy::disallowed_types)]
//!
//! # Usage
//!
//! ```rust,ignore
//! use aura_sync::{
//!     protocols::{AntiEntropyProtocol, JournalSyncProtocol},
//!     core::{SyncConfig, MetricsCollector, SessionManager},
//! };
//! use aura_core::effects::{NetworkEffects, JournalEffects, CryptoEffects};
//!
//! // Create sync configuration
//! let config = SyncConfig::for_production();
//!
//! // Set up metrics collection
//! let metrics = MetricsCollector::new();
//!
//! // Create session manager for protocol coordination
//! let session_manager = SessionManager::new(config.sessions());
//!
//! // Use protocols with any effect implementation
//! async fn sync_with_peer<E>(
//!     effects: &E,
//!     peer: DeviceId,
//! ) -> SyncResult<()>
//! where
//!     E: NetworkEffects + JournalEffects + CryptoEffects,
//! {
//!     let protocol = AntiEntropyProtocol::new(&config);
//!     protocol.sync_with_peer(effects, peer).await
//! }
//! ```

// Missing docs are temporarily allowed while protocol surfaces stabilize.
#![allow(missing_docs)]
#![forbid(unsafe_code)]

// =============================================================================
// Core Foundation Modules
// =============================================================================

/// Core abstractions and unified patterns for sync protocols
///
/// This module provides the foundational types and patterns used throughout
/// all sync protocols: unified error handling, message frameworks, configuration
/// management, metrics collection, and session coordination.
pub mod core;

/// Infrastructure utilities for sync operations
///
/// This module provides supporting infrastructure including peer management,
/// connection pooling, retry logic, cache management, and rate limiting.
/// All components follow Layer 5 patterns with effect-based interfaces.
pub mod infrastructure;

/// Protocol implementations for synchronization
///
/// This module provides complete end-to-end protocol implementations including
/// anti-entropy, journal sync, snapshots, OTA upgrades, and receipt verification.
/// All protocols follow Layer 5 patterns and are effect-based.
pub mod protocols;

/// Service layer for sync operations
///
/// This module provides high-level services that orchestrate protocols and
/// infrastructure to provide complete synchronization functionality.
/// All services implement the unified Service trait.
pub mod services;

/// Verification module for Merkle-based fact verification
///
/// This module provides cryptographic verification of facts during synchronization
/// using Merkle trees and Bloom filters from the IndexedJournalEffects.
pub mod verification;

/// Operation category map (A/B/C) for protocol gating and review.
pub const OPERATION_CATEGORIES: &[(&str, &str)] = &[
    ("sync:anti-entropy", "A"),
    ("sync:journal-sync", "A"),
    ("sync:snapshot", "A"),
    ("sync:ota-ceremony", "C"),
    ("sync:receipt-verify", "A"),
];

/// Lookup the operation category (A/B/C) for a given operation.
pub fn operation_category(operation: &str) -> Option<&'static str> {
    OPERATION_CATEGORIES
        .iter()
        .find(|(op, _)| *op == operation)
        .map(|(_, category)| *category)
}

// Re-export core types for convenience
pub use core::{
    MetricsCollector, SessionManager, SessionResult, SessionState, SyncConfig, SyncError,
    SyncResult,
};

// Protocol re-exports
pub use protocols::{WriterFence, WriterFenceGuard};

// Services re-exports
pub use services::maintenance;

// Verification re-exports
pub use verification::{MerkleComparison, MerkleVerifier, VerificationResult, VerificationStats};

// =============================================================================
// Integration Documentation
// =============================================================================

/// Integration documentation and patterns
///
/// Contains comprehensive documentation on how aura-sync integrates with
/// other crates in the Aura ecosystem, following the 8-layer architecture.
#[doc = include_str!("INTEGRATION.md")]
pub mod integration_docs {
    // This module exists only to include the integration documentation
    // in the generated rustdoc. The actual integration patterns are
    // implemented throughout the crate.
}

// =============================================================================
// Layer Dependencies Re-exports
// =============================================================================

// Re-export essential foundation types from Layer 1 (aura-core)
pub use aura_core::{AuraError, AuraResult, SessionId};

// Note: Other layer dependencies are imported as needed but not re-exported
// to maintain clean API boundaries and avoid dependency pollution.
// Users should import from the appropriate layer crates directly.
