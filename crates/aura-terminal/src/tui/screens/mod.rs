//! # Screens
//!
//! Full-screen views using iocraft components.

mod app;
mod block;
mod chat;
mod contacts;
mod help;
mod invitations;
mod neighborhood;
mod recovery;
mod router;
mod settings;

pub use app::{run_app_with_context, IoApp};
pub use block::{
    run_block_screen, BlockInviteCallback, BlockNavCallback, BlockScreen, BlockSendCallback,
};
pub use chat::{run_chat_screen, ChannelSelectCallback, ChatFocus, ChatScreen, SendCallback};
pub use contacts::{
    run_contacts_screen, ContactsScreen, StartChatCallback, ToggleGuardianCallback,
    UpdatePetnameCallback,
};
pub use help::{run_help_screen, HelpCommand, HelpScreen};
pub use invitations::{run_invitations_screen, InvitationCallback, InvitationsScreen};
pub use neighborhood::{
    run_neighborhood_screen, GoHomeCallback, NavigationCallback, NeighborhoodScreen,
};
pub use recovery::{run_recovery_screen, RecoveryCallback, RecoveryScreen};
pub use router::{NavAction, Router, Screen};
pub use settings::{run_settings_screen, MfaCallback, SettingsScreen};
