//! # View State Module
//!
//! This module contains the view state types that represent the current
//! application state. These types are FFI-safe and can be:
//!
//! - Serialized for debugging
//! - Passed to UniFFI for mobile
//! - Used with futures-signals for reactive updates

mod state;

pub mod block;
pub mod chat;
pub mod contacts;
pub mod invitations;
pub mod neighborhood;
pub mod recovery;

pub use state::ViewState;

// Re-export state types for convenience
pub use block::BlockState;
pub use chat::{Channel, ChannelType, ChatState, Message};
pub use contacts::ContactsState;
pub use invitations::InvitationsState;
pub use neighborhood::NeighborhoodState;
pub use recovery::RecoveryState;
