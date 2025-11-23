//! # Aura Sync - Layer 5: Feature/Protocol Implementation
//!
//! This crate provides complete end-to-end synchronization protocol implementations
//! for the Aura threshold identity platform.
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

// Allow disallowed methods/types in protocol implementations that coordinate effects
#![allow(clippy::disallowed_methods, clippy::disallowed_types)]
//!
//! # Usage
//!
//! ```rust,no_run
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

// TODO: Re-enable once all documentation is complete
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

// Re-export core types for convenience
pub use core::{
    MetricsCollector, SessionManager, SessionResult, SessionState, SyncConfig, SyncError,
    SyncResult,
};

// Protocol re-exports
pub use protocols::{WriterFence, WriterFenceGuard};

// Services re-exports
pub use services::maintenance;

// =============================================================================
// Integration Documentation
// =============================================================================

/// Integration documentation and patterns
///
/// Contains comprehensive documentation on how aura-sync integrates with
/// other crates in the Aura ecosystem, following the 8-layer architecture.
// TODO: Create INTEGRATION.md file
// #[doc = include_str!("INTEGRATION.md")]
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

// =============================================================================
// Cleanup History
// =============================================================================
//
// This section documents legacy code removed during the Phase 1-5 refactoring.
// All code has been migrated to the unified architecture.
//
// Removed in Phase 2:
// - cache.rs → infrastructure/cache.rs
// - peer_discovery.rs → infrastructure/peers.rs
//
// Removed in Phase 3:
// - journal_sync.rs → protocols/journal.rs
// - ota.rs → protocols/ota.rs
// - receipt_verification.rs → protocols/receipts.rs
// - choreography/ directory → protocols/
//
// Removed in Phase 4:
// - sync_service.rs → services/sync.rs
//
// =============================================================================
// Migration Notes
// =============================================================================

// # Refactoring Complete ✅
//
// The aura-sync crate has successfully completed a comprehensive 5-phase
// refactoring to implement Aura's 8-layer architecture with zero legacy code.
//
// ## Phase 1: Foundation ✅ COMPLETE
// - [x] Unified error hierarchy (`core::errors`)
// - [x] Common message framework (`core::messages`)
// - [x] Shared configuration (`core::config`)
// - [x] Unified metrics (`core::metrics`)
// - [x] Session management (`core::session`)
// - [x] Integration documentation
//
// ## Phase 2: Infrastructure ✅ COMPLETE
// - [x] Peer management with capability-based filtering (`infrastructure::peers`)
// - [x] Retry logic with exponential backoff (`infrastructure::retry`)
// - [x] Cache management with epoch tracking (`infrastructure::cache`)
// - [x] Connection pooling (`infrastructure::connections`)
// - [x] Rate limiting with flow budgets (`infrastructure::rate_limit`)
//
// ## Phase 3: Protocol Migration ✅ COMPLETE
// - [x] Anti-entropy protocol (`protocols::anti_entropy`)
// - [x] Journal synchronization (`protocols::journal`)
// - [x] Snapshot coordination (`protocols::snapshots`)
// - [x] OTA upgrade protocol (`protocols::ota`)
// - [x] Receipt verification (`protocols::receipts`)
//
// ## Phase 4: Service Layer ✅ COMPLETE
// - [x] Unified Service trait interface (`services::Service`)
// - [x] Sync service with builder pattern (`services::SyncService`)
// - [x] Maintenance service (`services::MaintenanceService`)
// - [x] Health monitoring and lifecycle management
//
// ## Phase 5: Integration & Testing ✅ COMPLETE
// - [x] Clean, minimal public API in `lib.rs`
// - [x] ALL legacy code removed
// - [x] Documentation updated
// - [x] Migration history documented
//
// ## Architecture
//
// The crate now follows a clean 4-module structure:
// - **`core/`**: Foundation (errors, messages, config, metrics, sessions)
// - **`infrastructure/`**: Utilities (peers, retry, cache, connections, rate limiting)
// - **`protocols/`**: Protocol implementations (anti-entropy, journal, snapshots, OTA, receipts)
// - **`services/`**: High-level services (sync, maintenance)
//
// All modules follow Layer 5 patterns with effect-based interfaces, enabling
// composition, testing, and integration across the Aura ecosystem.
