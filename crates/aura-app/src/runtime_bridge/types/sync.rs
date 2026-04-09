//! Sync-, rendezvous-, and runtime-status bridge types.

use aura_core::types::identifiers::AuthorityId;
use aura_core::DeviceId;

/// Status of the runtime's sync service.
#[derive(Debug, Clone, Default)]
pub struct SyncStatus {
    /// Whether the sync service is currently running.
    pub is_running: bool,
    /// Number of connected peers.
    pub connected_peers: usize,
    /// Last sync timestamp (milliseconds since epoch).
    pub last_sync_ms: Option<u64>,
    /// Pending facts waiting to be synced.
    pub pending_facts: usize,
    /// Number of active sync sessions (currently syncing with N peers).
    pub active_sessions: usize,
}

/// Status of the runtime's rendezvous service.
#[derive(Debug, Clone, Default)]
pub struct RendezvousStatus {
    /// Whether the rendezvous service is running.
    pub is_running: bool,
    /// Number of cached peers.
    pub cached_peers: usize,
}

/// Result of explicitly triggering peer discovery.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiscoveryTriggerOutcome {
    /// Discovery work was newly started by this request.
    Started,
    /// Discovery was already active; nothing new was started.
    AlreadyRunning,
}

/// Reachability refresh result after processing ceremony/contact traffic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReachabilityRefreshOutcome {
    /// Refresh completed successfully after processing progress.
    Refreshed,
    /// Refresh could not converge; callers must treat this as degraded state.
    Degraded {
        /// Human-readable degradation reason from the runtime-owned refresh path.
        reason: String,
    },
}

/// Counts for one ceremony/contact processing pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CeremonyProcessingCounts {
    /// Processed ceremony acceptances.
    pub acceptances: usize,
    /// Processed ceremony completions.
    pub completions: usize,
    /// Processed contact/channel invitation envelopes.
    pub contact_messages: usize,
    /// Processed rendezvous handshake envelopes.
    pub handshakes: usize,
}

impl CeremonyProcessingCounts {
    /// Total number of processed items across all categories.
    pub fn total(self) -> usize {
        self.acceptances
            .saturating_add(self.completions)
            .saturating_add(self.contact_messages)
            .saturating_add(self.handshakes)
    }
}

/// Outcome of one explicit ceremony/contact processing pass.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CeremonyProcessingOutcome {
    /// Nothing was available to process in this pass.
    NoProgress,
    /// Work was processed and any follow-up reachability refresh status is explicit.
    Processed {
        /// Counts by processed category.
        counts: CeremonyProcessingCounts,
        /// Reachability refresh result after processing progress.
        reachability_refresh: ReachabilityRefreshOutcome,
    },
}

/// Overall runtime status.
#[derive(Debug, Clone, Default)]
pub struct RuntimeStatus {
    /// Sync service status.
    pub sync: SyncStatus,
    /// Rendezvous service status.
    pub rendezvous: RendezvousStatus,
    /// Explicit authentication status.
    pub authentication: AuthenticationStatus,
}

/// Explicit runtime authentication status.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum AuthenticationStatus {
    /// No authenticated runtime authority/device is available.
    #[default]
    Unauthenticated,
    /// The runtime is authenticated for one concrete authority/device pair.
    Authenticated {
        /// Authenticated authority.
        authority_id: AuthorityId,
        /// Authenticated device.
        device_id: DeviceId,
    },
}
