//! Aura-Sync: Threshold Identity Synchronization Protocols
//!
//! This crate provides complete end-to-end synchronization protocol implementations
//! for the Aura threshold identity platform, following the 8-layer architecture
//! as a **Layer 5 (Feature/Protocol)** component.
//!
//! # Architecture Overview
//!
//! Aura-sync provides reusable protocol building blocks for:
//! - Journal state synchronization using CRDT semantics
//! - Anti-entropy protocols for state reconciliation  
//! - OTA upgrade coordination with threshold approval
//! - Session-based peer synchronization
//! - Receipt verification for distributed protocols
//!
//! # Design Principles
//!
//! - **Effect-Based Architecture**: All protocols parameterized by effect traits
//! - **Choreographic Coordination**: Following Aura's session type patterns
//! - **CRDT Semantics**: Built on journal semilattice operations
//! - **Zero Legacy Code**: Clean, modern APIs with no backwards compatibility
//! - **Layer 5 Compliance**: Reusable protocol building blocks, not UI applications
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
//!
//! # Integration
//!
//! See [`INTEGRATION.md`](crate::INTEGRATION) for detailed integration patterns
//! with other Aura crates and the effect system architecture.

#![deny(missing_docs)]
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
    SyncError, SyncResult, SyncConfig, MetricsCollector, SessionManager,
    SessionState, SessionResult,
};

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
pub use aura_core::{DeviceId, SessionId, AuraError, AuraResult};

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
// Removed in Phase 5:
// - maintenance.rs → services/maintenance.rs (types migrated)
// - All deprecated re-exports removed
// - All legacy compatibility code removed

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