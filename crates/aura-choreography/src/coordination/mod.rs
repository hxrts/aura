//! Infrastructure coordination choreographies

pub mod coordinator_monitor;
pub mod failure_recovery;
pub mod journal_sync_choreography;
pub mod session_epoch;

// Foundation types moved from aura-types
pub mod types_commit_reveal;
pub mod types_coordinator;
pub mod types_sync;

pub use coordinator_monitor::{CoordinatorMessage, CoordinatorMonitor};
pub use failure_recovery::CoordinatorFailureRecovery;
pub use journal_sync_choreography::SyncConfig;
// TODO: Re-enable when proper error handling is implemented
// pub use journal_sync_choreography::SyncError;
pub use journal_sync_choreography::{JournalSyncChoreography, SyncResult};
pub use session_epoch::{EpochBumpChoreography, EpochMessage, SessionEpochMonitor};

// Re-export foundation types
pub use types_commit_reveal::{CollectedCommitments, CollectedReveals, Commitment, Reveal};
pub use types_coordinator::{CoordinatorTimeout, Heartbeat};
pub use types_sync::SyncConfig as TypesSyncConfig;
