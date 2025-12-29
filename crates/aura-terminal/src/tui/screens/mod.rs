//! # Screens
//!
//! Full-screen views using iocraft components.
//!
//! Each screen is organized in its own directory with screen-specific modals colocated:
//! - `chat/` - ChatScreen, ChannelInfoModal, ChatCreateModal
//! - `notifications/` - NotificationsScreen
//! - `recovery/` - GuardianSetupModal, ThresholdModal

pub mod app;
mod chat;
mod contacts;
mod invitations;
mod neighborhood_v2;
mod notifications;
mod recovery;
mod router;
mod settings;

// Re-export callback types from centralized callbacks module
pub use crate::tui::callbacks::{
    AddDeviceCallback, ApprovalCallback, BlockSendCallback, ChannelSelectCallback,
    CreateAccountCallback, CreateChannelCallback, CreateInvitationCallback,
    ExportInvitationCallback, GoHomeCallback, GuardianSelectCallback, ImportInvitationCallback,
    InvitationCallback, RecoveryCallback, RemoveDeviceCallback, RetryMessageCallback, SendCallback,
    SetTopicCallback, StartChatCallback, UpdateDisplayNameCallback, UpdateNicknameCallback,
    UpdateThresholdCallback,
};

// Screen-specific callback types (use specialized types not in callbacks module)
pub use settings::MfaCallback;

// Screen components and runners
pub use app::{run_app_with_context, IoApp};
pub use chat::{run_chat_screen, ChatFocus, ChatScreen};
pub use contacts::{run_contacts_screen, ContactsScreen};
pub use neighborhood_v2::home_create_modal::HomeCreateModal;
pub use neighborhood_v2::{
    run_neighborhood_screen_v2, NeighborhoodScreenV2, NeighborhoodScreenV2Props,
};
pub use notifications::{run_notifications_screen, NotificationsScreen};
pub use router::{NavAction, Router, Screen};
pub use settings::{run_settings_screen, SettingsScreen};

// Screen-specific modals (re-exported from screen directories)
pub use chat::{ChannelInfoModal, ChatCreateModal, ChatCreateState, CreateChatCallback};
pub use invitations::{
    CancelCallback, ImportCallback, InvitationCodeModal, InvitationCodeState,
    InvitationCreateModal, InvitationImportModal, InvitationImportState,
    ModalCreateInvitationCallback,
};
pub use recovery::{
    GuardianCandidateProps, GuardianSetupKind, GuardianSetupModal, ThresholdModal, ThresholdState,
};
pub use settings::DeviceEnrollmentModal;
