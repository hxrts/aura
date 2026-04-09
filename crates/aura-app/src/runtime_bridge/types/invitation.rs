//! Invitation and authoritative-channel bridge types.

use aura_core::types::identifiers::{AuthorityId, CeremonyId, ChannelId, ContextId, InvitationId};
use aura_core::types::Epoch;
use aura_core::DeviceId;

/// Result of mutating invitation state through the runtime bridge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvitationMutationOutcome {
    /// Invitation that was mutated.
    pub invitation_id: InvitationId,
    /// The new canonical invitation status.
    pub new_status: InvitationBridgeStatus,
}

/// Bridge-level invitation type (for RuntimeBridge API).
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(missing_docs)]
pub enum InvitationBridgeType {
    Contact {
        nickname: Option<String>,
    },
    Guardian {
        subject_authority: AuthorityId,
    },
    Channel {
        home_id: String,
        context_id: Option<ContextId>,
        nickname_suggestion: Option<String>,
    },
    DeviceEnrollment {
        subject_authority: AuthorityId,
        initiator_device_id: DeviceId,
        device_id: DeviceId,
        nickname_suggestion: Option<String>,
        ceremony_id: CeremonyId,
        pending_epoch: Epoch,
    },
}

/// Bridge-level invitation status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InvitationBridgeStatus {
    /// Invitation is pending response.
    Pending,
    /// Invitation was accepted.
    Accepted,
    /// Invitation was declined.
    Declined,
    /// Invitation was cancelled by sender.
    Cancelled,
    /// Invitation has expired.
    Expired,
}

/// Bridge-level invitation info returned from `RuntimeBridge`.
#[derive(Debug, Clone)]
pub struct InvitationInfo {
    /// Unique invitation identifier (typed for type safety).
    pub invitation_id: InvitationId,
    /// Sender authority ID.
    pub sender_id: AuthorityId,
    /// Receiver authority ID.
    pub receiver_id: AuthorityId,
    /// Type of invitation.
    pub invitation_type: InvitationBridgeType,
    /// Current status.
    pub status: InvitationBridgeStatus,
    /// Creation timestamp (ms since epoch).
    pub created_at_ms: u64,
    /// Expiration timestamp (ms since epoch), if any.
    pub expires_at_ms: Option<u64>,
    /// Optional message from sender.
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
