//! # View State Module
//!
//! This module contains the view state types that represent the current
//! application state. These types are FFI-safe and can be:
//!
//! - Serialized for debugging
//! - Passed to UniFFI for mobile
//! - Used with futures-signals for reactive updates

mod state;

pub mod home;
pub mod chat;
pub mod contacts;
pub mod invitations;
pub mod neighborhood;
pub mod recovery;

pub use state::ViewState;

// Re-export state types for convenience
pub use home::{
    BanRecord, HomeState, HomesState, KickRecord, MuteRecord, Resident, ResidentRole,
};
pub use chat::{Channel, ChannelType, ChatState, Message};
pub use contacts::{Contact, ContactsState, MySuggestion, SuggestionPolicy};
pub use invitations::{
    Invitation, InvitationDirection, InvitationStatus, InvitationType, InvitationsState,
};
pub use neighborhood::{AdjacencyType, NeighborHome, NeighborhoodState, TraversalPosition};
pub use recovery::{
    Guardian, GuardianStatus, RecoveryApproval, RecoveryProcess, RecoveryProcessStatus,
    RecoveryState,
};
