//! Runtime bridge DTOs and offline-support state aliases.

#[cfg(test)]
use crate::core::IntentError;
use crate::views::naming::{truncate_id_for_display, EffectiveName};
use async_lock::Mutex;
use aura_core::threshold::{AgreementMode, ParticipantIdentity};
use aura_core::types::identifiers::{AuthorityId, CeremonyId, ChannelId, ContextId, InvitationId};
use aura_core::types::Epoch;
use aura_core::DeviceId;
use std::collections::HashMap;
use std::sync::Arc;

pub(crate) type PendingInvitationsState = Arc<Mutex<Option<Vec<InvitationInfo>>>>;
pub(crate) type AmpChannelContexts = Arc<Mutex<HashMap<ChannelId, ContextId>>>;
pub(crate) type MaterializedChannelNameMatches = Arc<Mutex<HashMap<String, Vec<ChannelId>>>>;
pub(crate) type AmpChannelStates = Arc<Mutex<HashMap<(ContextId, ChannelId), bool>>>;
pub(crate) type AmpChannelParticipants =
    Arc<Mutex<HashMap<(ContextId, ChannelId), Vec<AuthorityId>>>>;
pub(crate) type ModerationStatuses =
    Arc<Mutex<HashMap<(ContextId, ChannelId, AuthorityId), AuthoritativeModerationStatus>>>;
#[cfg(test)]
pub(crate) type OfflineAcceptInvitationResult =
    Arc<Mutex<Option<Result<InvitationMutationOutcome, IntentError>>>>;
#[cfg(test)]
pub(crate) type OfflineProcessCeremonyResult =
    Arc<Mutex<Option<Result<CeremonyProcessingOutcome, IntentError>>>>;

/// Status of the runtime's sync service
#[derive(Debug, Clone, Default)]
pub struct SyncStatus {
    /// Whether the sync service is currently running
    pub is_running: bool,
    /// Number of connected peers
    pub connected_peers: usize,
    /// Last sync timestamp (milliseconds since epoch)
    pub last_sync_ms: Option<u64>,
    /// Pending facts waiting to be synced
    pub pending_facts: usize,
    /// Number of active sync sessions (currently syncing with N peers)
    pub active_sessions: usize,
}

/// Status of the runtime's rendezvous service
#[derive(Debug, Clone, Default)]
pub struct RendezvousStatus {
    /// Whether the rendezvous service is running
    pub is_running: bool,
    /// Number of cached peers
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

/// Result of mutating invitation state through the runtime bridge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvitationMutationOutcome {
    /// Invitation that was mutated.
    pub invitation_id: InvitationId,
    /// The new canonical invitation status.
    pub new_status: InvitationBridgeStatus,
}

/// Overall runtime status
#[derive(Debug, Clone, Default)]
pub struct RuntimeStatus {
    /// Sync service status
    pub sync: SyncStatus,
    /// Rendezvous service status
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

/// High-level ceremony kind exposed across the runtime bridge boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CeremonyKind {
    /// Guardian threshold key rotation ceremony for an account authority.
    GuardianRotation,
    /// Device threshold key rotation ceremony (multifactor authority).
    DeviceRotation,
    /// Device enrollment ceremony (account authority membership change + rotation).
    DeviceEnrollment,
    /// Device removal ceremony (account authority membership change + rotation).
    DeviceRemoval,
    /// Guardian-based recovery ceremony.
    Recovery,
    /// Invitation ceremony (contact/guardian/channel).
    Invitation,
    /// Rendezvous secure-channel ceremony.
    RendezvousSecureChannel,
    /// OTA hard-fork activation ceremony.
    OtaActivation,
}

/// Result of starting a device enrollment ceremony.
#[derive(Debug, Clone)]
pub struct DeviceEnrollmentStart {
    /// Ceremony identifier for status polling / cancellation.
    pub ceremony_id: CeremonyId,
    /// Shareable enrollment code (e.g. QR/copy-paste) to import on the new device.
    pub enrollment_code: String,
    /// Pending epoch created during prepare.
    pub pending_epoch: Epoch,
    /// Device id being enrolled.
    pub device_id: DeviceId,
}

/// Status of a key-rotation / membership-change ceremony.
///
/// This is intentionally generic so multiple ceremony kinds can share the same
/// UI and workflow infrastructure.
#[derive(Debug, Clone)]
pub struct KeyRotationCeremonyStatus {
    /// Ceremony identifier
    pub ceremony_id: CeremonyId,
    /// What kind of ceremony this is
    pub kind: CeremonyKind,
    /// Number of participants who have accepted
    pub accepted_count: u16,
    /// Total number of required participants
    pub total_count: u16,
    /// Threshold required for completion
    pub threshold: u16,
    /// Whether the ceremony is complete
    pub is_complete: bool,
    /// Whether the ceremony has failed
    pub has_failed: bool,
    /// List of participants who have accepted
    pub accepted_participants: Vec<ParticipantIdentity>,
    /// Optional error message if failed
    pub error_message: Option<String>,
    /// Pending epoch for key rotation (if applicable)
    pub pending_epoch: Option<Epoch>,
    /// Agreement mode (A1/A2/A3)
    pub agreement_mode: AgreementMode,
    /// Whether reversion is still possible
    pub reversion_risk: bool,
}

/// Status of a guardian ceremony
#[derive(Debug, Clone)]
pub struct CeremonyStatus {
    /// Ceremony identifier
    pub ceremony_id: CeremonyId,
    /// Number of guardians who have accepted
    pub accepted_count: u16,
    /// Total number of guardians
    pub total_count: u16,
    /// Threshold required for completion
    pub threshold: u16,
    /// Whether the ceremony is complete
    pub is_complete: bool,
    /// Whether the ceremony has failed
    pub has_failed: bool,
    /// List of guardian IDs who have accepted
    pub accepted_guardians: Vec<AuthorityId>,
    /// Optional error message if failed
    pub error_message: Option<String>,
    /// Pending epoch for key rotation
    ///
    /// This is the epoch that was created when the ceremony started.
    /// If the ceremony is canceled, this epoch's keys should be rolled back.
    /// If the ceremony succeeds, this becomes the active epoch.
    pub pending_epoch: Option<Epoch>,
    /// Agreement mode (A1/A2/A3)
    pub agreement_mode: AgreementMode,
    /// Whether reversion is still possible
    pub reversion_risk: bool,
}

/// Discovery origin for a bootstrap candidate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BootstrapCandidateOrigin {
    /// Candidate discovered through native LAN discovery.
    Lan,
    /// Candidate discovered through a localhost bootstrap broker.
    LocalBroker,
    /// Candidate discovered through a LAN-visible bootstrap broker.
    LanBroker,
}

/// Information about a bootstrap candidate available for enrollment.
#[derive(Debug, Clone)]
pub struct BootstrapCandidateInfo {
    /// Authority ID of the discovered candidate.
    pub authority_id: AuthorityId,
    /// Discovery origin for this candidate.
    pub origin: BootstrapCandidateOrigin,
    /// Best currently known address for the candidate bootstrap path.
    pub address: String,
    /// When this candidate was discovered (ms since epoch).
    pub discovered_at_ms: u64,
    /// Nickname suggestion if available from the descriptor.
    pub nickname_suggestion: Option<String>,
}

// =============================================================================
// Invitation Bridge Types
// =============================================================================

/// Bridge-level invitation type (for RuntimeBridge API)
///
/// This is a minimal type for crossing the bridge boundary.
/// Workflows convert this to view types with display fields.
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(missing_docs)] // Field docs not required for bridge types
pub enum InvitationBridgeType {
    /// Contact invitation with optional nickname
    Contact { nickname: Option<String> },
    /// Guardian invitation for a subject authority
    Guardian { subject_authority: AuthorityId },
    /// Channel/home invitation with optional nickname suggestion
    Channel {
        home_id: String,
        context_id: Option<ContextId>,
        nickname_suggestion: Option<String>,
    },
    /// Device enrollment invitation (out-of-band transfer).
    DeviceEnrollment {
        subject_authority: AuthorityId,
        initiator_device_id: DeviceId,
        device_id: DeviceId,
        nickname_suggestion: Option<String>,
        ceremony_id: CeremonyId,
        pending_epoch: Epoch,
    },
}

/// Bridge-level invitation status
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InvitationBridgeStatus {
    /// Invitation is pending response
    Pending,
    /// Invitation was accepted
    Accepted,
    /// Invitation was declined
    Declined,
    /// Invitation was cancelled by sender
    Cancelled,
    /// Invitation has expired
    Expired,
}

/// Bridge-level invitation info returned from RuntimeBridge
///
/// Contains core invitation data without UI-specific display fields.
/// Workflows convert this to `views::invitations::Invitation` with resolved names.
#[derive(Debug, Clone)]
pub struct InvitationInfo {
    /// Unique invitation identifier (typed for type safety)
    pub invitation_id: InvitationId,
    /// Sender authority ID
    pub sender_id: AuthorityId,
    /// Receiver authority ID
    pub receiver_id: AuthorityId,
    /// Type of invitation
    pub invitation_type: InvitationBridgeType,
    /// Current status
    pub status: InvitationBridgeStatus,
    /// Creation timestamp (ms since epoch)
    pub created_at_ms: u64,
    /// Expiration timestamp (ms since epoch), if any
    pub expires_at_ms: Option<u64>,
    /// Optional message from sender
    pub message: Option<String>,
}

/// Canonical runtime-owned channel binding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AuthoritativeChannelBinding {
    /// Canonical channel id.
    pub channel_id: ChannelId,
    /// Canonical context id bound to that channel.
    pub context_id: ContextId,
}

/// Authoritative moderation status for an authority in a home-scoped context.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AuthoritativeModerationStatus {
    /// Whether the authority is banned from the home/context.
    pub is_banned: bool,
    /// Whether the authority is muted at the queried time.
    pub is_muted: bool,
    /// Whether the authoritative home roster is populated.
    pub roster_known: bool,
    /// Whether the authority is an authoritative member of the home roster.
    pub is_member: bool,
}

// =============================================================================
// Settings Bridge Types
// =============================================================================

/// Bridge-level settings state returned from RuntimeBridge
///
/// Contains persisted settings data. Device and contact lists
/// are derived views obtained from signals, not from here.
#[derive(Debug, Clone, Default)]
pub struct SettingsBridgeState {
    /// User's nickname suggestion (what they want to be called)
    pub nickname_suggestion: String,
    /// MFA policy setting
    pub mfa_policy: String,
    /// Threshold signing configuration (k of n)
    pub threshold_k: u16,
    /// Total guardians in threshold scheme
    pub threshold_n: u16,
    /// Number of registered devices
    pub device_count: usize,
    /// Number of contacts
    pub contact_count: usize,
}

impl SettingsBridgeState {
    /// Returns `true` if this state was populated from a real runtime.
    ///
    /// The `Default` implementation produces k=0, n=0 which is
    /// cryptographically invalid.  UI code should check this before
    /// displaying threshold information.
    pub fn has_valid_threshold(&self) -> bool {
        self.threshold_k >= 2 && self.threshold_n >= self.threshold_k
    }
}

/// Bridge-level device summary.
///
/// This is used to populate UI settings screens without requiring the UI layer
/// to understand commitment-tree internals.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BridgeDeviceInfo {
    /// Stable device identifier
    pub id: DeviceId,
    /// Human-friendly label (best effort, computed for display)
    pub name: String,
    /// Local nickname override (user-assigned name for this device)
    pub nickname: Option<String>,
    /// Nickname suggestion (what the device wants to be called, from enrollment)
    pub nickname_suggestion: Option<String>,
    /// Whether this is the current device
    pub is_current: bool,
    /// Last-seen timestamp (ms since epoch), if known
    pub last_seen: Option<u64>,
}

impl EffectiveName for BridgeDeviceInfo {
    fn nickname(&self) -> Option<&str> {
        self.nickname.as_deref().filter(|s| !s.is_empty())
    }

    fn nickname_suggestion(&self) -> Option<&str> {
        self.nickname_suggestion
            .as_deref()
            .filter(|s| !s.is_empty())
    }

    fn fallback_id(&self) -> String {
        truncate_id_for_display(&self.id.to_string())
    }
}

/// Bridge-level authority summary for settings and authority switching.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BridgeAuthorityInfo {
    /// Stable authority identifier.
    pub id: AuthorityId,
    /// Best-effort display label or nickname suggestion.
    pub nickname_suggestion: Option<String>,
    /// Whether this is the currently active authority for the runtime.
    pub is_current: bool,
}
