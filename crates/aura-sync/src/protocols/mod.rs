//! Layer 5: Synchronization Protocol Implementations - Anti-Entropy, Snapshots, OTA, Epochs
//!
//! Complete end-to-end protocol implementations built atop Layer 4 orchestration:
//! - **anti_entropy**: Digest-based CRDT reconciliation with state transfer (per docs/110_state_reduction.md)
//! - **journal**: Journal operation synchronization with causal ordering guarantees
//! - **snapshots**: Coordinated garbage collection with writer fencing and threshold approval
//! - **ota**: OTA upgrade coordination with epoch fencing for consistency
//! - **receipts**: Receipt verification for multi-hop message chains (per docs/003_information_flow_contract.md)
//! - **epochs**: Epoch rotation and identity epoch management with AMP consensus
//!
//! **Protocol Principles** (per docs/107_mpst_and_choreography.md):
//! - **Effect-based**: Parameterized by effect traits (NetworkEffects, JournalEffects) for testing
//! - **Choreographic**: Use session types (aura-mpst) for distributed coordination with deadlock freedom
//! - **Composable**: Can be combined without tight coupling via effect composition
//! - **Reusable**: Building blocks for services (aura-sync/services) and higher-level workflows
//! - **Guard-integrated**: Messages flow through guard chain (CapGuard → FlowGuard → Journal)
//!
//! ```rust,ignore
//! async fn sync_with_peer<E>(effects: &E, peer: DeviceId) -> SyncResult<()>
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
pub mod fact_sync;
pub mod journal;
pub mod ota;
pub mod ota_ceremony;
pub mod receipts;
pub mod snapshots;

// New authority-centric modules
pub mod authority_journal_sync;
pub mod namespaced_sync;

use aura_core::effects::{JournalEffects, NetworkEffects, PhysicalTimeEffects};

pub trait SyncJournalEffects: JournalEffects + Send + Sync {}

impl<T> SyncJournalEffects for T where T: JournalEffects + Send + Sync {}

pub trait SyncProtocolEffects: SyncJournalEffects + NetworkEffects + PhysicalTimeEffects {}

impl<T> SyncProtocolEffects for T where T: SyncJournalEffects + NetworkEffects + PhysicalTimeEffects {}

pub trait SyncCoreJournalEffects: JournalEffects + Send + Sync {}

impl<T> SyncCoreJournalEffects for T where T: JournalEffects + Send + Sync {}

pub trait SyncCoreProtocolEffects:
    SyncCoreJournalEffects + NetworkEffects + PhysicalTimeEffects
{
}

impl<T> SyncCoreProtocolEffects for T where
    T: SyncCoreJournalEffects + NetworkEffects + PhysicalTimeEffects
{
}

// Re-export key types for convenience
pub use anti_entropy::{
    AntiEntropyConfig, AntiEntropyProtocol, AntiEntropyRequest, AntiEntropyResult, DigestStatus,
    JournalDigest, LoggingProgressCallback, NoOpProgressCallback, SyncProgressCallback,
    SyncProgressEvent,
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

pub use ota_ceremony::{
    OTACeremonyConfig, OTACeremonyEffects, OTACeremonyExecutor, OTACeremonyFact, OTACeremonyId,
    OTACeremonyState, OTACeremonyStatus, ReadinessCommitment,
    UpgradeProposal as CeremonyUpgradeProposal,
};

pub use fact_sync::{FactSyncConfig, FactSyncProtocol, FactSyncResult, FactSyncStats};
