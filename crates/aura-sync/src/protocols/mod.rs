//! Protocol implementations for synchronization
//!
//! This module provides complete end-to-end synchronization protocol implementations
//! following Aura's Layer 5 (Feature/Protocol) architecture. Each protocol is:
//! - Effect-based: Parameterized by effect traits
//! - Choreographic: Uses session types where appropriate
//! - Composable: Can be combined with other protocols
//! - Reusable: Building blocks for higher-level services
//!
//! # Protocol Modules
//!
//! - `anti_entropy`: Digest-based state reconciliation for CRDT synchronization
//! - `journal`: Journal operation synchronization and coordination
//! - `snapshots`: Coordinated garbage collection with threshold approval
//! - `ota`: Over-the-air upgrade coordination with epoch fencing
//! - `receipts`: Receipt verification for multi-hop message chains
//! - `epochs`: Epoch rotation and identity epoch management
//!
//! # Architecture
//!
//! All protocols follow these patterns:
//!
//! ## Effect-Based Design
//!
//! ```rust,no_run
//! use aura_sync::protocols::AntiEntropyProtocol;
//! use aura_core::effects::{JournalEffects, NetworkEffects};
//!
//! async fn sync<E>(effects: &E, peer: DeviceId) -> SyncResult<()>
//! where
//!     E: JournalEffects + NetworkEffects,
//! {
//!     let protocol = AntiEntropyProtocol::new(config);
//!     protocol.execute(effects, peer).await
//! }
//! ```
//!
//! ## Infrastructure Integration
//!
//! Protocols use infrastructure from Phase 2:
//! - `RetryPolicy` for resilient operations
//! - `PeerManager` for peer selection
//! - `ConnectionPool` for connection management
//! - `RateLimiter` for flow budget enforcement
//!
//! ## Clean Public APIs
//!
//! Each protocol exposes:
//! - Configuration types
//! - Result types
//! - Main execution interface
//! - Statistics and metrics

pub mod anti_entropy;
pub mod epochs;
pub mod journal;
pub mod ota;
pub mod receipts;
pub mod snapshots;

// New authority-centric modules
pub mod authority_journal_sync;
pub mod namespaced_sync;

// Re-export key types for convenience
pub use anti_entropy::{
    AntiEntropyConfig, AntiEntropyProtocol, AntiEntropyRequest, AntiEntropyResult, DigestStatus,
    JournalDigest,
};

pub use journal::{
    JournalSyncConfig, JournalSyncProtocol, JournalSyncResult, SyncMessage, SyncState,
};

pub use snapshots::{
    SnapshotApproval, SnapshotConfig, SnapshotProposal, SnapshotProtocol, SnapshotResult,
    WriterFence, WriterFenceGuard,
};

pub use ota::{OTAConfig, OTAProtocol, OTAResult, UpgradeKind, UpgradeProposal};

pub use receipts::{ReceiptVerificationConfig, ReceiptVerificationProtocol, VerificationResult};

pub use epochs::{
    EpochCommit, EpochConfig, EpochConfirmation, EpochRotation, EpochRotationCoordinator,
    EpochRotationProposal, RotationStatus,
};
