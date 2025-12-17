//! # Screens
//!
//! Full-screen views using iocraft components.
//!
//! Each screen is organized in its own directory with screen-specific modals colocated:
//! - `chat/` - ChatScreen, ChannelInfoModal, ChatCreateModal
//! - `invitations/` - InvitationsScreen, InvitationCodeModal, InvitationCreateModal, InvitationImportModal
//! - `recovery/` - RecoveryScreen, GuardianSetupModal, ThresholdModal

mod app;
mod block;
mod chat;
mod contacts;
mod invitations;
mod neighborhood;
mod recovery;
mod router;
mod settings;

// Re-export callback types from centralized callbacks module
pub use crate::tui::callbacks::{
    AddDeviceCallback, ApprovalCallback, BlockInviteCallback, BlockNavCallback, BlockSendCallback,
    ChannelSelectCallback, CreateAccountCallback, CreateChannelCallback, CreateInvitationCallback,
    ExportInvitationCallback, GoHomeCallback, GrantStewardCallback, GuardianSelectCallback,
    ImportInvitationCallback, InvitationCallback, RecoveryCallback, RemoveDeviceCallback,
    RetryMessageCallback, RevokeStewardCallback, SendCallback, SetTopicCallback, StartChatCallback,
    UpdateNicknameCallback, UpdatePetnameCallback, UpdateThresholdCallback,
};

// Screen-specific callback types (use specialized types not in callbacks module)
pub use neighborhood::NavigationCallback;
pub use settings::MfaCallback;

// Screen components and runners
pub use app::{run_app_with_context, IoApp};
pub use block::{run_block_screen, BlockFocus, BlockScreen};
pub use chat::{run_chat_screen, ChatFocus, ChatScreen};
pub use contacts::{run_contacts_screen, ContactsScreen};
pub use invitations::InvitationsScreen;
pub use neighborhood::{run_neighborhood_screen, NeighborhoodScreen};
pub use recovery::{run_recovery_screen, RecoveryScreen};
pub use router::{NavAction, Router, Screen};
pub use settings::{run_settings_screen, SettingsScreen};

// Screen-specific modals (re-exported from screen directories)
pub use chat::{ChannelInfoModal, ChatCreateModal, ChatCreateState, CreateChatCallback};
pub use invitations::{
    CancelCallback, ImportCallback, InvitationCodeModal, InvitationCodeState, InvitationCreateModal,
    InvitationCreateState, InvitationImportModal, InvitationImportState,
    ModalCreateInvitationCallback,
};
pub use recovery::{GuardianCandidateProps, GuardianSetupModal, ThresholdModal, ThresholdState};
