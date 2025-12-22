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

/// Focus for two-panel screens (list + detail layout)
///
/// Used by screens with a left panel (list) and right panel (detail view).
/// Navigation: Left/Right (h/l) toggles between panels, Up/Down (j/k) navigates within.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PanelFocus {
    #[default]
    List,
    Detail,
}

impl PanelFocus {
    /// Toggle between list and detail focus
    pub fn toggle(self) -> Self {
        match self {
            PanelFocus::List => PanelFocus::Detail,
            PanelFocus::Detail => PanelFocus::List,
        }
    }

    /// Check if list panel is focused
    pub fn is_list(self) -> bool {
        matches!(self, PanelFocus::List)
    }

    /// Check if detail panel is focused
    pub fn is_detail(self) -> bool {
        matches!(self, PanelFocus::Detail)
    }
}
