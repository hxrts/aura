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
pub mod journal;
pub mod snapshots;
pub mod ota;
pub mod receipts;

// Note: epochs.rs will be added when migrated from aura-protocol

// Re-export key types for convenience
pub use anti_entropy::{
    AntiEntropyProtocol, AntiEntropyConfig, AntiEntropyResult,
    JournalDigest, DigestStatus, AntiEntropyRequest,
};

pub use journal::{
    JournalSyncProtocol, JournalSyncConfig, JournalSyncResult,
    SyncState, SyncMessage,
};

pub use snapshots::{
    SnapshotProtocol, SnapshotConfig, SnapshotResult,
    SnapshotProposal, SnapshotApproval,
    WriterFence, WriterFenceGuard,
};

pub use ota::{
    OTAProtocol, OTAConfig, OTAResult,
    UpgradeProposal, UpgradeKind,
};

pub use receipts::{
    ReceiptVerificationProtocol, ReceiptVerificationConfig,
    VerificationResult,
};
