//! # State Snapshot
//!
//! An FFI-safe snapshot of the current application state.
//! This is used for initial state retrieval and for platforms
//! that prefer polling over reactive updates.

use crate::views::{
    BlockState, BlocksState, ChatState, ContactsState, InvitationsState, NeighborhoodState,
    RecoveryState,
};
use serde::{Deserialize, Serialize};

/// A complete snapshot of the application state.
///
/// This is FFI-safe and can be serialized for debugging.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct StateSnapshot {
    /// Chat state (channels, messages, unread counts)
    pub chat: ChatState,

    /// Recovery state (guardians, recovery status)
    pub recovery: RecoveryState,

    /// Invitations state (pending, sent, received)
    pub invitations: InvitationsState,

    /// Contacts state (contacts, petnames)
    pub contacts: ContactsState,

    /// Block state (residents, storage, settings)
    pub block: BlockState,

    /// Multi-block state (all blocks the user has created/joined)
    pub blocks: BlocksState,

    /// Neighborhood state (adjacent blocks, traversal)
    pub neighborhood: NeighborhoodState,
}

impl StateSnapshot {
    /// Create an empty snapshot
    pub fn empty() -> Self {
        Self::default()
    }

    /// Check if the snapshot represents an uninitialized state
    pub fn is_empty(&self) -> bool {
        self.chat.channels.is_empty()
            && self.recovery.guardians.is_empty()
            && self.invitations.pending.is_empty()
            && self.contacts.contacts.is_empty()
            && self.blocks.is_empty()
    }
}
