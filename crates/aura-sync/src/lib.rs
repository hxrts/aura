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
pub use aura_core::{DeviceId, SessionId, AuraError, AuraResult};

// Note: Other layer dependencies are imported as needed but not re-exported
// to maintain clean API boundaries and avoid dependency pollution.
// Users should import from the appropriate layer crates directly.

// =============================================================================
// TEMPORARY: Legacy Module Access
// =============================================================================
// 
// During the refactoring process, we temporarily expose the legacy modules
// to prevent downstream compilation failures. These will be removed in Phase 5
// after all protocols have been migrated to the new unified architecture.
//
// 🚨 WARNING: All items below are DEPRECATED and will be removed in Phase 5
// 🚨 Do not use these in new code - use the unified protocols instead

#[deprecated(note = "Legacy sync service - migrate to unified protocols in Phase 2")]
#[doc(hidden)]
pub mod sync_service;

// Removed in Phase 2: cache.rs replaced with infrastructure/cache.rs

#[deprecated(note = "Legacy journal sync - migrate to protocols module in Phase 3")]
#[doc(hidden)]
pub mod journal_sync;

// Removed in Phase 2: peer_discovery.rs placeholder replaced with infrastructure/peers.rs

#[deprecated(note = "Legacy OTA module - migrate to protocols module in Phase 3")]
#[doc(hidden)]
pub mod ota;

#[deprecated(note = "Legacy maintenance module - migrate to services module in Phase 4")]
#[doc(hidden)]
pub mod maintenance;

#[deprecated(note = "Legacy receipt verification - migrate to protocols module in Phase 3")]
#[doc(hidden)]
pub mod receipt_verification;

#[deprecated(note = "Legacy choreography module - migrate to unified protocols in Phase 3")]
#[doc(hidden)]
pub mod choreography;

// Legacy re-exports (deprecated - will be removed in Phase 5)
#[deprecated(note = "Use core::SyncError instead")]
pub use aura_protocol::effects::SyncError as LegacySyncError;

#[deprecated(note = "Use infrastructure::CacheEpochTracker instead")]
pub use infrastructure::cache::CacheEpochTracker as CacheEpochFloors;

#[deprecated(note = "Use unified protocols and services instead")]
pub use maintenance::{
    AdminReplaced, CacheInvalidated, CacheKey, IdentityEpochFence, MaintenanceEvent,
    SnapshotCompleted, SnapshotProposed, UpgradeActivated, UpgradeKind, UpgradeProposal,
};

#[deprecated(note = "Use unified protocols instead")]
pub use ota::{UpgradeCoordinator, UpgradeReadiness};

#[deprecated(note = "Use unified services instead")]
pub use sync_service::SyncService;

// Legacy choreography re-exports (deprecated)
#[deprecated(note = "Use unified protocols instead")]
pub use choreography::journal::{
    OpLogSynchronizer, PeerSyncError, PeerSyncManager, PeerSyncState, ProtocolError,
    SchedulerConfig, SchedulerError, SyncConfiguration, SyncMessage, SyncProtocol,
    SyncResult as LegacySyncResult, SyncScheduler, SyncState,
};

#[deprecated(note = "Use unified protocols instead")]
pub use choreography::{
    snapshot::{
        AbortReason, SnapshotAbort, SnapshotApplicationResult, SnapshotApproval,
        SnapshotCommit, SnapshotError, SnapshotManager, SnapshotProposal, SnapshotRejection,
        SnapshotResult, ThresholdSnapshotConfig, ThresholdSnapshotCoordinator, TreeStateDigest,
        WriterFence,
    },
    tree_coordination::{
        ApprovalRequest, ApprovalResponse, ThresholdApprovalConfig, ThresholdApprovalCoordinator,
        ValidationRequest, ValidationResponse, ValidationResult,
    },
    tree_sync::{
        DeltaRequest, DeltaResponse, HubConfig, MeshConfig, PeerSyncConfig, StateRequest,
        StateResponse, SyncComplete, TreeDelta, TreeSyncCoordinator,
    },
};

// Silence deprecation warnings for the legacy re-exports during migration
#[allow(deprecated)]
pub use choreography::snapshot::SessionId as LegacySessionId;

// =============================================================================
// Migration Notes
// =============================================================================

//! # Migration Roadmap
//!
//! This crate is currently undergoing a comprehensive refactoring to implement
//! Aura's 8-layer architecture. The migration follows this timeline:
//!
//! ## Phase 1: Foundation ✅ COMPLETE
//! - [x] Unified error hierarchy (`core::errors`)
//! - [x] Common message framework (`core::messages`)  
//! - [x] Shared configuration (`core::config`)
//! - [x] Unified metrics (`core::metrics`)
//! - [x] Session management (`core::session`)
//! - [x] Integration documentation
//! - [x] CLEANUP GATE: Remove legacy public APIs
//!
//! ## Phase 2: Infrastructure (IN PROGRESS)
//! - [ ] Consolidate effect handlers and transport
//! - [ ] Migrate peer management to unified patterns
//! - [ ] Create infrastructure modules
//! - [ ] Remove scattered infrastructure code
//!
//! ## Phase 3: Protocol Migration  
//! - [ ] Migrate journal sync to unified protocols
//! - [ ] Migrate anti-entropy to unified patterns
//! - [ ] Migrate OTA upgrade coordination  
//! - [ ] Migrate receipt verification
//! - [ ] Remove legacy protocol modules
//!
//! ## Phase 4: Service Layer
//! - [ ] Refactor sync service to use unified patterns
//! - [ ] Create maintenance service
//! - [ ] Implement unified service interfaces
//! - [ ] Remove legacy service code
//!
//! ## Phase 5: Integration & Testing
//! - [ ] Create minimal public API
//! - [ ] Remove ALL legacy code
//! - [ ] Update documentation
//! - [ ] Comprehensive integration tests
//! - [ ] Performance benchmarking
//! - [ ] Final API review
//!
//! ## Current Status: Phase 1 Complete ✅
//!
//! The foundation has been laid with unified core modules. All new development
//! should use the `core` module patterns. Legacy modules are deprecated and
//! will be progressively replaced and removed.