//! View state types for TUI screens
//!
//! This module contains the state structs for each screen in the TUI.

mod account_setup;
mod block;
mod chat;
mod contacts;
mod guardian;
mod help;
mod invitations;
mod neighborhood;
mod recovery;
mod settings;

pub use account_setup::*;
pub use block::*;
pub use chat::*;
pub use contacts::*;
pub use guardian::*;
pub use help::*;
pub use invitations::*;
pub use neighborhood::*;
pub use recovery::*;
pub use settings::*;

/// Focus for two-panel screens
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PanelFocus {
    #[default]
    List,
    Detail,
}
