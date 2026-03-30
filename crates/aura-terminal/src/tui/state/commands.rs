//! TUI Command types
//!
//! Commands produced by state transitions to be executed by the runtime.

use crate::tui::screens::Screen;
use crate::tui::types::{AccessLevel, MfaPolicy};
use aura_core::AuthorityId;

use super::toast::ToastLevel;
use super::{CeremonyId, ChannelId, ContactId, DeviceId, HomeId, InvitationId};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InvitationKind {
    Guardian,
    Contact,
    Channel,
}

impl InvitationKind {
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Guardian => "guardian",
            Self::Contact => "contact",
            Self::Channel => "channel",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HomeTarget {
    Current,
    Home,
    Explicit(HomeId),
}

impl HomeTarget {
    #[must_use]
    pub fn from_input(value: impl Into<String>) -> Self {
        let value = value.into();
        match value.trim().to_ascii_lowercase().as_str() {
            "current" => Self::Current,
            "home" => Self::Home,
            _ => Self::Explicit(value.into()),
        }
    }

    #[must_use]
    pub fn as_command_arg(&self) -> String {
        match self {
            Self::Current => "current".to_string(),
            Self::Home => "home".to_string(),
            Self::Explicit(home_id) => home_id.to_string(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ThresholdK(u8);

impl ThresholdK {
    pub fn new(value: u8) -> Result<Self, String> {
        if value == 0 {
            return Err("Threshold must be at least 1".to_string());
        }
        Ok(Self(value))
    }

    #[must_use]
    pub const fn get(self) -> u8 {
        self.0
    }
}

impl TryFrom<u8> for ThresholdK {
    type Error = String;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HomeCapabilityConfig {
    full: Vec<String>,
    partial: Vec<String>,
    limited: Vec<String>,
}

impl HomeCapabilityConfig {
    pub fn parse(full_caps: &str, partial_caps: &str, limited_caps: &str) -> Result<Self, String> {
        let parse_caps = |raw: &str, label: &str| -> Result<Vec<String>, String> {
            let parsed: Vec<String> = raw
                .split(',')
                .map(str::trim)
                .filter(|cap| !cap.is_empty())
                .map(ToString::to_string)
                .collect();
            if parsed.is_empty() {
                return Err(format!("{label} capability set cannot be empty"));
            }
            Ok(parsed)
        };

        Ok(Self {
            full: parse_caps(full_caps, "Full")?,
            partial: parse_caps(partial_caps, "Partial")?,
            limited: parse_caps(limited_caps, "Limited")?,
        })
    }

    #[must_use]
    pub fn full_csv(&self) -> String {
        self.full.join(",")
    }

    #[must_use]
    pub fn partial_csv(&self) -> String {
        self.partial.join(",")
    }

    #[must_use]
    pub fn limited_csv(&self) -> String {
        self.limited.join(",")
    }
}

/// Command representing a side effect
///
/// Commands are produced by state transitions and executed by the runtime.
/// They represent all effects that cannot be handled purely.
#[derive(Clone, Debug)]
pub enum TuiCommand {
    /// Exit the TUI
    Exit,

    /// Show a toast notification
    ShowToast { message: String, level: ToastLevel },

    /// Dismiss a toast notification
    DismissToast { id: u64 },

    /// Clear all toast notifications (e.g., on Escape)
    ClearAllToasts,

    /// Dispatch an effect command to the app core
    Dispatch(DispatchCommand),

    /// Harness-only follow-up that removes the targeted or currently visible non-current device.
    HarnessRemoveVisibleDevice { device_id: Option<String> },

    /// Request a re-render
    Render,
}

/// Commands to dispatch to the app core
#[derive(Clone, Debug)]
pub enum DispatchCommand {
    // Navigation
    NavigateTo(Screen),

    // Chat screen
    SelectChannel {
        channel_id: ChannelId,
    },
    AcceptPendingHomeInvitation,
    JoinChannel {
        channel_name: String,
    },
    SendChatMessage {
        content: String,
    },
    RetryMessage,
    OpenChatTopicModal,
    OpenChatInfoModal,
    OpenChatCreateWizard,
    CreateChannel {
        name: String,
        topic: Option<String>,
        members: Vec<AuthorityId>,
        threshold_k: ThresholdK,
    },
    SetChannelTopic {
        channel_id: ChannelId,
        topic: String,
    },
    DeleteChannel {
        channel_id: ChannelId,
    },

    // Contacts screen
    UpdateNickname {
        contact_id: ContactId,
        nickname: String,
    },
    /// Open the “edit nickname” modal for the currently-selected contact.
    ///
    /// The shell populates the modal with the selected contact ID and current nickname
    /// from reactive subscriptions.
    OpenContactNicknameModal,
    /// Open the “create invitation” modal.
    ///
    /// The shell populates the modal with the selected receiver (contact/peer)
    /// from reactive subscriptions.
    OpenCreateInvitationModal,
    SendSelectedFriendRequest,
    AcceptSelectedFriendRequest,
    DeclineSelectedFriendRequest,
    RevokeSelectedFriendship,
    InviteSelectedContactToChannel,
    InviteActorToChannel {
        authority_id: AuthorityId,
        channel_id: String,
    },
    /// Invite the currently selected LAN peer.
    InviteLanPeer,
    StartChat,
    RemoveContact {
        contact_id: ContactId,
    },
    /// Open remove contact confirmation modal (shell populates selected contact)
    OpenRemoveContactModal,
    /// Contact selection by index (for generic contact select modals)
    SelectContactByIndex {
        index: usize,
    },

    // Guardian ceremony
    /// Open guardian setup modal (shell will populate contacts)
    OpenGuardianSetup,
    /// Open MFA setup modal (shell will populate devices)
    OpenMfaSetup,
    /// Start a guardian ceremony with selected contacts and threshold
    StartGuardianCeremony {
        contact_ids: Vec<AuthorityId>,
        threshold_k: ThresholdK,
    },
    /// Start an MFA ceremony with selected devices and threshold
    StartMfaCeremony {
        device_ids: Vec<DeviceId>,
        threshold_k: ThresholdK,
    },
    /// Cancel an in-progress guardian ceremony
    CancelGuardianCeremony {
        ceremony_id: CeremonyId,
    },
    /// Cancel an in-progress key rotation ceremony (device enrollment, guardian rotation, etc.).
    CancelKeyRotationCeremony {
        ceremony_id: CeremonyId,
    },

    // Invitations screen
    AcceptInvitation,
    DeclineInvitation,
    CreateInvitation {
        receiver_id: AuthorityId,
        invitation_type: InvitationKind,
        message: Option<String>,
        ttl_secs: Option<u64>,
    },
    ImportInvitation {
        code: String,
    },
    ExportInvitation,
    RevokeInvitation {
        invitation_id: InvitationId,
    },

    // Recovery screen
    StartRecovery,
    AddGuardian {
        contact_id: ContactId,
    },
    ApproveRecovery,

    // Settings screen
    UpdateNicknameSuggestion {
        nickname_suggestion: String,
    },
    UpdateMfaPolicy {
        policy: MfaPolicy,
    },
    AddDevice {
        name: String,
        /// Invitee's authority ID for addressed device enrollment.
        invitee_authority_id: AuthorityId,
    },
    RemoveDevice {
        device_id: DeviceId,
    },
    /// Open device selection modal (for device removal)
    OpenDeviceSelectModal,
    /// Import a device enrollment code on the target device runtime.
    /// In demo mode this routes to the simulated Mobile agent.
    ImportDeviceEnrollmentOnMobile {
        code: String,
    },
    /// Import a device enrollment code while completing onboarding.
    /// Success must always dismiss the onboarding flow.
    ImportDeviceEnrollmentDuringOnboarding {
        code: String,
    },
    /// Open authority picker modal (for switching between authorities)
    OpenAuthorityPicker,
    /// Switch to a different authority
    SwitchAuthority {
        authority_id: AuthorityId,
    },

    // Neighborhood screen
    EnterHome,
    GoHome,
    BackToLimited,
    /// Open home creation flow
    OpenHomeCreate,
    /// Open moderator assignment/revocation modal
    OpenModeratorAssignmentModal,
    /// Submit moderator assignment/revocation for the selected member
    SubmitModeratorAssignment {
        target_id: AuthorityId,
        assign: bool,
    },
    /// Open per-user access override modal
    OpenAccessOverrideModal,
    /// Submit per-user access override
    SubmitAccessOverride {
        target_id: AuthorityId,
        access_level: AccessLevel,
    },
    /// Open home capability configuration modal
    OpenHomeCapabilityConfigModal,
    /// Submit home capability configuration
    SubmitHomeCapabilityConfig {
        config: HomeCapabilityConfig,
    },
    /// Create a new home
    CreateHome {
        name: String,
        description: Option<String>,
    },
    /// Create/select active neighborhood
    CreateNeighborhood {
        name: String,
    },
    /// Add selected home as member of active neighborhood
    AddSelectedHomeToNeighborhood,
    /// Add an explicit home ID as member of active neighborhood
    AddHomeToNeighborhood {
        target: HomeTarget,
    },
    /// Link selected home one_hop_link as direct
    LinkSelectedHomeOneHopLink,
    /// Link explicit home one_hop_link as direct
    LinkHomeOneHopLink {
        target: HomeTarget,
    },

    // Account setup
    CreateAccount {
        name: String,
    },
}
