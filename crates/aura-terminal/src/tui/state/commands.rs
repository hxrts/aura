//! TUI Command types
//!
//! Commands produced by state transitions to be executed by the runtime.

use crate::tui::screens::Screen;
use crate::tui::types::MfaPolicy;

use super::toast::ToastLevel;

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

    /// Request a re-render
    Render,
}

/// Commands to dispatch to the app core
#[derive(Clone, Debug)]
pub enum DispatchCommand {
    // Navigation
    NavigateTo(Screen),

    // Block screen
    SendBlockMessage {
        content: String,
    },
    InviteToBlock {
        contact_id: String,
    },
    /// Open block invite modal (shell will populate contacts)
    OpenBlockInvite,
    GrantStewardSelected,
    RevokeStewardSelected,

    // Chat screen
    SelectChannel {
        channel_id: String,
    },
    SendChatMessage {
        content: String,
    },
    RetryMessage,
    OpenChatTopicModal,
    OpenChatInfoModal,
    OpenChatMemberSelect,
    CreateChannel {
        name: String,
        topic: Option<String>,
        members: Vec<String>,
    },
    SetChannelTopic {
        channel_id: String,
        topic: String,
    },
    DeleteChannel {
        channel_id: String,
    },

    // Contacts screen
    UpdateNickname {
        contact_id: String,
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
    StartChat,
    RemoveContact {
        contact_id: String,
    },
    /// Contact selection by index (for generic contact select modals)
    SelectContactByIndex {
        index: usize,
    },

    // Guardian ceremony
    /// Open guardian setup modal (shell will populate contacts)
    OpenGuardianSetup,
    /// Start a guardian ceremony with selected contacts and threshold
    StartGuardianCeremony {
        contact_ids: Vec<String>,
        threshold_k: u8,
    },
    /// Cancel an in-progress guardian ceremony
    CancelGuardianCeremony {
        ceremony_id: String,
    },
    /// Cancel an in-progress key rotation ceremony (device enrollment, guardian rotation, etc.).
    CancelKeyRotationCeremony {
        ceremony_id: String,
    },

    // Invitations screen
    AcceptInvitation,
    DeclineInvitation,
    CreateInvitation {
        receiver_id: String,
        invitation_type: String,
        message: Option<String>,
        ttl_secs: Option<u64>,
    },
    ImportInvitation {
        code: String,
    },
    ExportInvitation,
    RevokeInvitation {
        invitation_id: String,
    },

    // Recovery screen
    StartRecovery,
    AddGuardian {
        contact_id: String,
    },
    ApproveRecovery,

    // Settings screen
    UpdateDisplayName {
        display_name: String,
    },
    UpdateMfaPolicy {
        policy: MfaPolicy,
    },
    AddDevice {
        name: String,
    },
    RemoveDevice {
        device_id: String,
    },

    // Neighborhood screen
    EnterBlock,
    GoHome,
    BackToStreet,

    // Account setup
    CreateAccount {
        name: String,
    },
}
