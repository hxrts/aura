//! Invitation Workflow - Portable Business Logic
//!
//! This module contains invitation operations that are portable across
//! all frontends via the RuntimeBridge abstraction.
//!
//! ## TTL Presets
//!
//! Standard TTL presets for invitation expiration:
//! - 1 hour: Quick invitations
//! - 24 hours (default): Standard invitations
//! - 1 week (168 hours): Extended invitations
//! - 30 days (720 hours): Long-term invitations

use crate::runtime_bridge::{InvitationBridgeType, InvitationInfo};

// ============================================================================
// TTL Constants
// ============================================================================

/// 1 hour TTL preset in hours
pub const INVITATION_TTL_1_HOUR: u64 = 1;

/// 1 day (24 hours) TTL preset in hours
pub const INVITATION_TTL_1_DAY: u64 = 24;

/// 1 week (168 hours) TTL preset in hours
pub const INVITATION_TTL_1_WEEK: u64 = 168;

/// 30 days (720 hours) TTL preset in hours
pub const INVITATION_TTL_30_DAYS: u64 = 720;

/// Standard TTL presets in hours: 1h, 1d, 1w, 30d
pub const INVITATION_TTL_PRESETS: [u64; 4] = [
    INVITATION_TTL_1_HOUR,
    INVITATION_TTL_1_DAY,
    INVITATION_TTL_1_WEEK,
    INVITATION_TTL_30_DAYS,
];

/// Default TTL for invitations (24 hours)
pub const DEFAULT_INVITATION_TTL_HOURS: u64 = INVITATION_TTL_1_DAY;

/// Convert TTL from hours to milliseconds.
///
/// # Examples
///
/// ```ignore
/// use aura_app::workflows::invitation::ttl_hours_to_ms;
///
/// assert_eq!(ttl_hours_to_ms(1), 3_600_000);   // 1 hour
/// assert_eq!(ttl_hours_to_ms(24), 86_400_000); // 24 hours
/// ```
#[inline]
#[must_use]
pub const fn ttl_hours_to_ms(hours: u64) -> u64 {
    hours * 60 * 60 * 1000
}

/// Format TTL for human-readable display.
///
/// Returns a user-friendly string representation of the TTL duration.
///
/// # Examples
///
/// ```ignore
/// use aura_app::workflows::invitation::format_ttl_display;
///
/// assert_eq!(format_ttl_display(1), "1 hour");
/// assert_eq!(format_ttl_display(24), "1 day");
/// assert_eq!(format_ttl_display(168), "1 week");
/// assert_eq!(format_ttl_display(720), "30 days");
/// ```
#[must_use]
pub fn format_ttl_display(hours: u64) -> String {
    match hours {
        0 => "No expiration".to_string(),
        1 => "1 hour".to_string(),
        h if h < 24 => format!("{h} hours"),
        24 => "1 day".to_string(),
        h if h < 168 => {
            let days = h / 24;
            if days == 1 {
                "1 day".to_string()
            } else {
                format!("{days} days")
            }
        }
        168 => "1 week".to_string(),
        h if h < 720 => {
            let weeks = h / 168;
            if weeks == 1 {
                "1 week".to_string()
            } else {
                format!("{weeks} weeks")
            }
        }
        720 => "30 days".to_string(),
        h => {
            let days = h / 24;
            format!("{days} days")
        }
    }
}

/// Get the TTL preset index for a given hours value.
///
/// Returns the index in `INVITATION_TTL_PRESETS` that matches or is closest
/// to the given hours value.
#[must_use]
pub fn ttl_preset_index(hours: u64) -> usize {
    INVITATION_TTL_PRESETS
        .iter()
        .position(|&preset| preset == hours)
        .unwrap_or(1) // Default to 24h (index 1)
}

/// Get the next TTL preset from the current hours value.
///
/// Cycles through presets: 1h -> 24h -> 1w -> 30d -> 1h
#[must_use]
pub fn next_ttl_preset(current_hours: u64) -> u64 {
    let current_index = ttl_preset_index(current_hours);
    let next_index = (current_index + 1) % INVITATION_TTL_PRESETS.len();
    INVITATION_TTL_PRESETS[next_index]
}

/// Get the previous TTL preset from the current hours value.
///
/// Cycles through presets: 1h <- 24h <- 1w <- 30d <- 1h
#[must_use]
pub fn prev_ttl_preset(current_hours: u64) -> u64 {
    let current_index = ttl_preset_index(current_hours);
    let prev_index = if current_index == 0 {
        INVITATION_TTL_PRESETS.len() - 1
    } else {
        current_index - 1
    };
    INVITATION_TTL_PRESETS[prev_index]
}
use crate::signal_defs::INVITATIONS_SIGNAL;
use crate::workflows::runtime::require_runtime;
#[cfg(feature = "signals")]
use crate::workflows::signals::read_signal;
use crate::workflows::signals::read_signal_or_default;
use crate::{views::invitations::InvitationsState, AppCore};
use async_lock::RwLock;
use aura_core::effects::amp::ChannelBootstrapPackage;
#[cfg(feature = "signals")]
use aura_core::effects::reactive::ReactiveEffects;
use aura_core::identifiers::{AuthorityId, InvitationId};
use aura_core::AuraError;
use std::sync::Arc;

#[cfg(feature = "signals")]
async fn yield_once() {
    use std::future::Future;
    use std::pin::Pin;
    use std::task::{Context, Poll};

    struct YieldOnce(bool);

    impl Future for YieldOnce {
        type Output = ();

        fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
            if self.0 {
                Poll::Ready(())
            } else {
                self.0 = true;
                cx.waker().wake_by_ref();
                Poll::Pending
            }
        }
    }

    YieldOnce(false).await;
}

// ============================================================================
// Invitation Creation via RuntimeBridge
// ============================================================================

/// Create a contact invitation
///
/// **What it does**: Creates an invitation to become a contact
/// **Returns**: InvitationInfo with the created invitation details
/// **Signal pattern**: RuntimeBridge handles state updates
pub async fn create_contact_invitation(
    app_core: &Arc<RwLock<AppCore>>,
    receiver: AuthorityId,
    nickname: Option<String>,
    message: Option<String>,
    ttl_ms: Option<u64>,
) -> Result<InvitationInfo, AuraError> {
    let runtime = require_runtime(app_core).await?;

    runtime
        .create_contact_invitation(receiver, nickname, message, ttl_ms)
        .await
        .map_err(|e| AuraError::agent(format!("Failed to create contact invitation: {e}")))
}

/// Create a guardian invitation
///
/// **What it does**: Creates an invitation to become a guardian
/// **Returns**: InvitationInfo with the created invitation details
/// **Signal pattern**: RuntimeBridge handles state updates
pub async fn create_guardian_invitation(
    app_core: &Arc<RwLock<AppCore>>,
    receiver: AuthorityId,
    subject: AuthorityId,
    message: Option<String>,
    ttl_ms: Option<u64>,
) -> Result<InvitationInfo, AuraError> {
    let runtime = require_runtime(app_core).await?;

    runtime
        .create_guardian_invitation(receiver, subject, message, ttl_ms)
        .await
        .map_err(|e| AuraError::agent(format!("Failed to create guardian invitation: {e}")))
}

/// Create a channel invitation
///
/// **What it does**: Creates an invitation to join a channel
/// **Returns**: InvitationInfo with the created invitation details
/// **Signal pattern**: RuntimeBridge handles state updates
pub async fn create_channel_invitation(
    app_core: &Arc<RwLock<AppCore>>,
    receiver: AuthorityId,
    home_id: String,
    bootstrap: Option<ChannelBootstrapPackage>,
    message: Option<String>,
    ttl_ms: Option<u64>,
) -> Result<InvitationInfo, AuraError> {
    let runtime = require_runtime(app_core).await?;

    runtime
        .create_channel_invitation(receiver, home_id, bootstrap, message, ttl_ms)
        .await
        .map_err(|e| AuraError::agent(format!("Failed to create channel invitation: {e}")))
}

// ============================================================================
// Invitation Queries via RuntimeBridge
// ============================================================================

/// List pending invitations via RuntimeBridge
///
/// **What it does**: Gets all pending invitations from the RuntimeBridge
/// **Returns**: Vector of InvitationInfo
/// **Signal pattern**: Read-only operation (no emission)
pub async fn list_pending_invitations(app_core: &Arc<RwLock<AppCore>>) -> Vec<InvitationInfo> {
    let runtime = {
        let core = app_core.read().await;
        match core.runtime() {
            Some(r) => r.clone(),
            None => return Vec::new(),
        }
    };

    runtime.list_pending_invitations().await
}

/// Import and get invitation details from a shareable code
///
/// **What it does**: Parses invitation code and returns the details
/// **Returns**: InvitationInfo with parsed details
/// **Signal pattern**: Read-only until acceptance
pub async fn import_invitation_details(
    app_core: &Arc<RwLock<AppCore>>,
    code: &str,
) -> Result<InvitationInfo, AuraError> {
    let runtime = require_runtime(app_core).await?;

    runtime
        .import_invitation(code)
        .await
        .map_err(|e| AuraError::agent(format!("Failed to import invitation: {e}")))
}

// ============================================================================
// Export Operations via RuntimeBridge
// ============================================================================

/// Export an invitation code for sharing
///
/// **What it does**: Generates shareable invitation code
/// **Returns**: Base64-encoded invitation code
/// **Signal pattern**: Read-only operation (no emission)
///
/// This method is implemented via RuntimeBridge.export_invitation().
/// Takes a typed InvitationId, returns the shareable invitation code as String.
pub async fn export_invitation(
    app_core: &Arc<RwLock<AppCore>>,
    invitation_id: &InvitationId,
) -> Result<String, AuraError> {
    let runtime = require_runtime(app_core).await?;

    runtime
        .export_invitation(invitation_id.as_str())
        .await
        .map_err(|e| AuraError::agent(format!("Failed to export invitation: {e}")))
}

/// Export an invitation by string ID (legacy/convenience API).
pub async fn export_invitation_by_str(
    app_core: &Arc<RwLock<AppCore>>,
    invitation_id: &str,
) -> Result<String, AuraError> {
    export_invitation(app_core, &InvitationId::new(invitation_id)).await
}

/// Get current invitations state
///
/// **What it does**: Reads invitation state from INVITATIONS_SIGNAL
/// **Returns**: Current invitations (sent and received)
/// **Signal pattern**: Read-only operation (no emission)
pub async fn list_invitations(app_core: &Arc<RwLock<AppCore>>) -> InvitationsState {
    read_signal_or_default(app_core, &*INVITATIONS_SIGNAL).await
}

// ============================================================================
// Invitation Operations via RuntimeBridge
// ============================================================================

/// Accept an invitation
///
/// **What it does**: Accepts a received invitation via RuntimeBridge using typed InvitationId
/// **Returns**: Unit result
/// **Signal pattern**: RuntimeBridge handles signal emission
pub async fn accept_invitation(
    app_core: &Arc<RwLock<AppCore>>,
    invitation_id: &InvitationId,
) -> Result<(), AuraError> {
    let runtime = require_runtime(app_core).await?;

    #[cfg(feature = "signals")]
    let initial_contact_count = {
        read_signal(
            app_core,
            &*crate::signal_defs::CONTACTS_SIGNAL,
            crate::signal_defs::CONTACTS_SIGNAL_NAME,
        )
        .await
        .unwrap_or_default()
        .contact_count()
    };

    #[cfg(feature = "signals")]
    let mut contacts_stream = {
        let core = app_core.read().await;
        core.subscribe(&*crate::signal_defs::CONTACTS_SIGNAL)
    };

    runtime
        .accept_invitation(invitation_id.as_str())
        .await
        .map_err(|e| AuraError::agent(format!("Failed to accept invitation: {e}")))?;

    // Give the runtime fact pipeline a bounded chance to publish CONTACTS_SIGNAL before we refresh
    // derived UI signals like CONNECTION_STATUS_SIGNAL.
    #[cfg(feature = "signals")]
    {
        for _ in 0..4096 {
            // Prefer consuming emissions if available (fast path).
            if let Some(state) = contacts_stream.try_recv() {
                if state.contact_count() > initial_contact_count {
                    break;
                }
            } else {
                // Fallback: check current state (covers missed emissions).
                let contacts_len = read_signal(
                    app_core,
                    &*crate::signal_defs::CONTACTS_SIGNAL,
                    crate::signal_defs::CONTACTS_SIGNAL_NAME,
                )
                .await
                .unwrap_or_default()
                .contact_count();

                if contacts_len > initial_contact_count {
                    break;
                }
            }

            yield_once().await;
        }
    }

    // Best-effort: refresh signals so UI status (e.g. online contact count) updates immediately.
    let _ = super::system::refresh_account(app_core).await;

    Ok(())
}

/// Accept an invitation by string ID (legacy/convenience API).
pub async fn accept_invitation_by_str(
    app_core: &Arc<RwLock<AppCore>>,
    invitation_id: &str,
) -> Result<(), AuraError> {
    accept_invitation(app_core, &InvitationId::new(invitation_id)).await
}

/// Decline an invitation using typed InvitationId
///
/// **What it does**: Declines a received invitation via RuntimeBridge
/// **Returns**: Unit result
/// **Signal pattern**: RuntimeBridge handles signal emission
pub async fn decline_invitation(
    app_core: &Arc<RwLock<AppCore>>,
    invitation_id: &InvitationId,
) -> Result<(), AuraError> {
    let runtime = require_runtime(app_core).await?;

    runtime
        .decline_invitation(invitation_id.as_str())
        .await
        .map_err(|e| AuraError::agent(format!("Failed to decline invitation: {e}")))
}

/// Decline an invitation by string ID (legacy/convenience API).
pub async fn decline_invitation_by_str(
    app_core: &Arc<RwLock<AppCore>>,
    invitation_id: &str,
) -> Result<(), AuraError> {
    decline_invitation(app_core, &InvitationId::new(invitation_id)).await
}

/// Cancel an invitation using typed InvitationId
///
/// **What it does**: Cancels a sent invitation via RuntimeBridge
/// **Returns**: Unit result
/// **Signal pattern**: RuntimeBridge handles signal emission
pub async fn cancel_invitation(
    app_core: &Arc<RwLock<AppCore>>,
    invitation_id: &InvitationId,
) -> Result<(), AuraError> {
    let runtime = require_runtime(app_core).await?;

    runtime
        .cancel_invitation(invitation_id.as_str())
        .await
        .map_err(|e| AuraError::agent(format!("Failed to cancel invitation: {e}")))
}

/// Cancel an invitation by string ID (legacy/convenience API).
pub async fn cancel_invitation_by_str(
    app_core: &Arc<RwLock<AppCore>>,
    invitation_id: &str,
) -> Result<(), AuraError> {
    cancel_invitation(app_core, &InvitationId::new(invitation_id)).await
}

/// Import an invitation from a shareable code
///
/// **What it does**: Parses and validates invitation code via RuntimeBridge
/// **Returns**: Unit result
/// **Signal pattern**: RuntimeBridge handles signal emission
///
/// The code parsing and validation is handled by the RuntimeBridge implementation.
pub async fn import_invitation(
    app_core: &Arc<RwLock<AppCore>>,
    code: &str,
) -> Result<(), AuraError> {
    let runtime = require_runtime(app_core).await?;

    runtime
        .import_invitation(code)
        .await
        .map(|_| ()) // Discard InvitationInfo, just return success
        .map_err(|e| AuraError::agent(format!("Failed to import invitation: {e}")))
}

// ============================================================================
// Invitation Role Parsing and Formatting
// ============================================================================

use crate::views::invitations::InvitationType;

/// Portable invitation role value for CLI parsing.
///
/// This enum represents the user-facing role categories for invitation creation.
/// It maps to the underlying `InvitationType` but includes additional context
/// like whether it's a "contact" (default) invitation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InvitationRoleValue {
    /// Contact invitation (default for unknown roles)
    Contact {
        /// Optional nickname for the contact
        nickname: Option<String>,
    },
    /// Guardian invitation
    Guardian,
    /// Channel/Chat invitation
    Channel,
}

impl InvitationRoleValue {
    /// Get the canonical string representation.
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Contact { .. } => "contact",
            Self::Guardian => "guardian",
            Self::Channel => "channel",
        }
    }

    /// Convert to `InvitationType`.
    #[must_use]
    pub fn to_invitation_type(&self) -> InvitationType {
        match self {
            Self::Contact { .. } => InvitationType::Home,
            Self::Guardian => InvitationType::Guardian,
            Self::Channel => InvitationType::Chat,
        }
    }
}

impl std::fmt::Display for InvitationRoleValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Contact {
                nickname: Some(name),
            } => write!(f, "contact (nickname: {name})"),
            Self::Contact { nickname: None } => write!(f, "contact"),
            Self::Guardian => write!(f, "guardian"),
            Self::Channel => write!(f, "channel"),
        }
    }
}

/// Parse an invitation role string into a portable value.
///
/// Recognizes "guardian" and "channel" (case-insensitive).
/// Any other value is treated as a contact role with the input as nickname.
///
/// # Examples
///
/// ```ignore
/// use aura_app::workflows::invitation::parse_invitation_role;
///
/// // Known roles
/// let guardian = parse_invitation_role("guardian");
/// assert!(matches!(guardian, InvitationRoleValue::Guardian));
///
/// let channel = parse_invitation_role("CHANNEL");
/// assert!(matches!(channel, InvitationRoleValue::Channel));
///
/// // Unknown roles become contact with nickname
/// let custom = parse_invitation_role("friend");
/// assert!(matches!(custom, InvitationRoleValue::Contact { nickname: Some(n) } if n == "friend"));
/// ```
#[must_use]
pub fn parse_invitation_role(role: &str) -> InvitationRoleValue {
    if role.eq_ignore_ascii_case("guardian") {
        InvitationRoleValue::Guardian
    } else if role.eq_ignore_ascii_case("channel") {
        InvitationRoleValue::Channel
    } else {
        // Default: treat as contact with optional nickname
        let nickname = if role.is_empty() {
            None
        } else {
            Some(role.to_string())
        };
        InvitationRoleValue::Contact { nickname }
    }
}

/// Format an invitation type for human-readable display.
///
/// Provides consistent formatting of invitation types across all frontends.
#[must_use]
pub fn format_invitation_type(inv_type: InvitationType) -> &'static str {
    match inv_type {
        InvitationType::Home => "Home",
        InvitationType::Guardian => "Guardian",
        InvitationType::Chat => "Channel",
    }
}

/// Format an invitation type with additional context.
///
/// For more detailed formatting that includes context like channel IDs or authorities.
#[must_use]
pub fn format_invitation_type_detailed(inv_type: InvitationType, context: Option<&str>) -> String {
    match (inv_type, context) {
        (InvitationType::Home, None) => "Home".to_string(),
        (InvitationType::Home, Some(ctx)) => format!("Home ({ctx})"),
        (InvitationType::Guardian, None) => "Guardian".to_string(),
        (InvitationType::Guardian, Some(ctx)) => format!("Guardian (for: {ctx})"),
        (InvitationType::Chat, None) => "Channel".to_string(),
        (InvitationType::Chat, Some(ctx)) => format!("Channel ({ctx})"),
    }
}

// ============================================================================
// Additional Invitation Operations
// ============================================================================

/// Accept the first pending home/channel invitation
///
/// **What it does**: Finds and accepts the first pending channel invitation
/// **Returns**: Invitation ID that was accepted
/// **Signal pattern**: RuntimeBridge handles signal emission
///
/// This is used by UI to quickly accept a pending home invitation without
/// requiring the user to select a specific invitation ID.
/// Returns the typed InvitationId of the accepted invitation.
pub async fn accept_pending_home_invitation(
    app_core: &Arc<RwLock<AppCore>>,
) -> Result<InvitationId, AuraError> {
    let runtime = require_runtime(app_core).await?;

    // Get pending invitations
    let pending = runtime.list_pending_invitations().await;

    // Find a channel invitation that we received (sender is not us)
    let our_authority = runtime.authority_id();
    let home_invitation = pending.iter().find(|inv| {
        matches!(inv.invitation_type, InvitationBridgeType::Channel { .. })
            && inv.sender_id != our_authority
    });

    match home_invitation {
        Some(inv) => {
            runtime
                .accept_invitation(inv.invitation_id.as_str())
                .await
                .map_err(|e| AuraError::agent(format!("Failed to accept invitation: {e}")))?;
            Ok(inv.invitation_id.clone())
        }
        None => Err(AuraError::agent("No pending home invitation found")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AppConfig;

    // === Invitation Role Parsing Tests ===

    #[test]
    fn test_parse_invitation_role_guardian() {
        let result = parse_invitation_role("guardian");
        assert_eq!(result, InvitationRoleValue::Guardian);
    }

    #[test]
    fn test_parse_invitation_role_guardian_case_insensitive() {
        assert_eq!(
            parse_invitation_role("GUARDIAN"),
            InvitationRoleValue::Guardian
        );
        assert_eq!(
            parse_invitation_role("Guardian"),
            InvitationRoleValue::Guardian
        );
    }

    #[test]
    fn test_parse_invitation_role_channel() {
        let result = parse_invitation_role("channel");
        assert_eq!(result, InvitationRoleValue::Channel);
    }

    #[test]
    fn test_parse_invitation_role_channel_case_insensitive() {
        assert_eq!(
            parse_invitation_role("CHANNEL"),
            InvitationRoleValue::Channel
        );
        assert_eq!(
            parse_invitation_role("Channel"),
            InvitationRoleValue::Channel
        );
    }

    #[test]
    fn test_parse_invitation_role_contact_default() {
        let result = parse_invitation_role("friend");
        assert!(matches!(
            result,
            InvitationRoleValue::Contact { nickname: Some(n) } if n == "friend"
        ));
    }

    #[test]
    fn test_parse_invitation_role_empty_is_contact_no_nickname() {
        let result = parse_invitation_role("");
        assert!(matches!(
            result,
            InvitationRoleValue::Contact { nickname: None }
        ));
    }

    #[test]
    fn test_invitation_role_as_str() {
        assert_eq!(InvitationRoleValue::Guardian.as_str(), "guardian");
        assert_eq!(InvitationRoleValue::Channel.as_str(), "channel");
        assert_eq!(
            InvitationRoleValue::Contact { nickname: None }.as_str(),
            "contact"
        );
    }

    #[test]
    fn test_invitation_role_display() {
        assert_eq!(format!("{}", InvitationRoleValue::Guardian), "guardian");
        assert_eq!(format!("{}", InvitationRoleValue::Channel), "channel");
        assert_eq!(
            format!("{}", InvitationRoleValue::Contact { nickname: None }),
            "contact"
        );
        assert_eq!(
            format!(
                "{}",
                InvitationRoleValue::Contact {
                    nickname: Some("Alice".to_string())
                }
            ),
            "contact (nickname: Alice)"
        );
    }

    #[test]
    fn test_invitation_role_to_invitation_type() {
        assert_eq!(
            InvitationRoleValue::Guardian.to_invitation_type(),
            InvitationType::Guardian
        );
        assert_eq!(
            InvitationRoleValue::Channel.to_invitation_type(),
            InvitationType::Chat
        );
        assert_eq!(
            InvitationRoleValue::Contact { nickname: None }.to_invitation_type(),
            InvitationType::Home
        );
    }

    #[test]
    fn test_format_invitation_type() {
        assert_eq!(format_invitation_type(InvitationType::Home), "Home");
        assert_eq!(format_invitation_type(InvitationType::Guardian), "Guardian");
        assert_eq!(format_invitation_type(InvitationType::Chat), "Channel");
    }

    #[test]
    fn test_format_invitation_type_detailed() {
        assert_eq!(
            format_invitation_type_detailed(InvitationType::Home, None),
            "Home"
        );
        assert_eq!(
            format_invitation_type_detailed(InvitationType::Home, Some("living room")),
            "Home (living room)"
        );
        assert_eq!(
            format_invitation_type_detailed(InvitationType::Guardian, Some("alice-authority")),
            "Guardian (for: alice-authority)"
        );
        assert_eq!(
            format_invitation_type_detailed(InvitationType::Chat, Some("general")),
            "Channel (general)"
        );
    }

    // === TTL Tests ===

    #[test]
    fn test_ttl_constants() {
        assert_eq!(INVITATION_TTL_1_HOUR, 1);
        assert_eq!(INVITATION_TTL_1_DAY, 24);
        assert_eq!(INVITATION_TTL_1_WEEK, 168);
        assert_eq!(INVITATION_TTL_30_DAYS, 720);
        assert_eq!(DEFAULT_INVITATION_TTL_HOURS, 24);
    }

    #[test]
    fn test_ttl_presets_array() {
        assert_eq!(INVITATION_TTL_PRESETS.len(), 4);
        assert_eq!(INVITATION_TTL_PRESETS[0], 1);
        assert_eq!(INVITATION_TTL_PRESETS[1], 24);
        assert_eq!(INVITATION_TTL_PRESETS[2], 168);
        assert_eq!(INVITATION_TTL_PRESETS[3], 720);
    }

    #[test]
    fn test_ttl_hours_to_ms() {
        assert_eq!(ttl_hours_to_ms(1), 3_600_000);
        assert_eq!(ttl_hours_to_ms(24), 86_400_000);
        assert_eq!(ttl_hours_to_ms(168), 604_800_000);
        assert_eq!(ttl_hours_to_ms(720), 2_592_000_000);
    }

    #[test]
    fn test_format_ttl_display_presets() {
        assert_eq!(format_ttl_display(1), "1 hour");
        assert_eq!(format_ttl_display(24), "1 day");
        assert_eq!(format_ttl_display(168), "1 week");
        assert_eq!(format_ttl_display(720), "30 days");
    }

    #[test]
    fn test_format_ttl_display_other_values() {
        assert_eq!(format_ttl_display(0), "No expiration");
        assert_eq!(format_ttl_display(2), "2 hours");
        assert_eq!(format_ttl_display(12), "12 hours");
        assert_eq!(format_ttl_display(48), "2 days");
        assert_eq!(format_ttl_display(336), "2 weeks");
        assert_eq!(format_ttl_display(1000), "41 days");
    }

    #[test]
    fn test_ttl_preset_index() {
        assert_eq!(ttl_preset_index(1), 0);
        assert_eq!(ttl_preset_index(24), 1);
        assert_eq!(ttl_preset_index(168), 2);
        assert_eq!(ttl_preset_index(720), 3);
        // Unknown value defaults to index 1 (24h)
        assert_eq!(ttl_preset_index(100), 1);
    }

    #[test]
    fn test_next_ttl_preset() {
        assert_eq!(next_ttl_preset(1), 24);
        assert_eq!(next_ttl_preset(24), 168);
        assert_eq!(next_ttl_preset(168), 720);
        assert_eq!(next_ttl_preset(720), 1); // Wraps around
    }

    #[test]
    fn test_prev_ttl_preset() {
        assert_eq!(prev_ttl_preset(1), 720); // Wraps around
        assert_eq!(prev_ttl_preset(24), 1);
        assert_eq!(prev_ttl_preset(168), 24);
        assert_eq!(prev_ttl_preset(720), 168);
    }

    // === Workflow Tests ===

    #[tokio::test]
    async fn test_list_invitations_default() {
        let config = AppConfig::default();
        let app_core = Arc::new(RwLock::new(AppCore::new(config).unwrap()));

        let invitations = list_invitations(&app_core).await;
        assert_eq!(invitations.sent_count(), 0);
        assert_eq!(invitations.pending_count(), 0);
    }
}
