//! # Screens
//!
//! Full-screen views using iocraft components.

mod app;
mod block;
mod chat;
mod contacts;
mod invitations;
mod neighborhood;
mod recovery;
mod router;
mod settings;

pub use app::{run_app_with_context, IoApp};
pub use block::{
    run_block_screen, BlockFocus, BlockInviteCallback, BlockNavCallback, BlockScreen,
    BlockSendCallback, GrantStewardCallback, RevokeStewardCallback,
};
pub use chat::{
    run_chat_screen, ChannelSelectCallback, ChatFocus, ChatScreen, CreateChannelCallback,
    RetryMessageCallback, SendCallback, SetTopicCallback,
};
pub use contacts::{
    run_contacts_screen, ContactsScreen, ImportInvitationCallback, StartChatCallback,
    UpdatePetnameCallback,
};
pub use invitations::{
    CreateInvitationCallback, ExportInvitationCallback, InvitationCallback, InvitationsScreen,
};
pub use neighborhood::{
    run_neighborhood_screen, GoHomeCallback, NavigationCallback, NeighborhoodScreen,
};
pub use recovery::{run_recovery_screen, RecoveryCallback, RecoveryScreen};
pub use router::{NavAction, Router, Screen};
pub use settings::{
    run_settings_screen, AddDeviceCallback, MfaCallback, RemoveDeviceCallback, SettingsScreen,
    UpdateNicknameCallback, UpdateThresholdCallback,
};
