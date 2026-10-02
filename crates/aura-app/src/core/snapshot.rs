//! # State Snapshot
//!
//! An FFI-safe snapshot of the current application state.
//! This is used for initial state retrieval and for platforms
//! that prefer polling over reactive updates.

use crate::views::{
    ChatState, ContactsState, HomesState, InvitationsState, NeighborhoodState, RecoveryState,
};
use serde::{Deserialize, Serialize};

/// Revisions of the graph values copied into this view snapshot. These source
/// revisions are independent of a frontend's semantic or render revision.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct ProjectionSourceRevisions {
    /// Last chat graph revision copied into the app view.
    pub chat: Option<u64>,
    /// Last contacts graph revision copied into the app view.
    pub contacts: Option<u64>,
    /// Last homes graph revision copied into the app view.
    pub homes: Option<u64>,
    /// Last invitations graph revision copied into the app view.
    pub invitations: Option<u64>,
    /// Last recovery graph revision copied into the app view.
    pub recovery: Option<u64>,
    /// Last neighborhood graph revision copied into the app view.
    pub neighborhood: Option<u64>,
}

/// A complete snapshot of the application state.
///
/// This is FFI-safe and can be serialized for debugging.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct StateSnapshot {
    /// Source graph revisions for the observed projection values below.
    #[serde(default)]
    pub projection_source_revisions: ProjectionSourceRevisions,
    /// Chat state (channels, messages, unread counts)
    pub chat: ChatState,

    /// Recovery state (guardians, recovery status)
    pub recovery: RecoveryState,

    /// Invitations state (pending, sent, received)
    pub invitations: InvitationsState,

    /// Contacts state (contacts, nicknames)
    pub contacts: ContactsState,

    /// Multi-home state (all homes the user has created/joined)
    pub homes: HomesState,

    /// Neighborhood state (adjacent homes, traversal)
    pub neighborhood: NeighborhoodState,
}

impl StateSnapshot {
    /// Create an empty snapshot
    pub fn empty() -> Self {
        Self::default()
    }

    /// Check if the snapshot represents an uninitialized state
    pub fn is_empty(&self) -> bool {
        self.chat.is_empty()
            && self.recovery.guardian_count() == 0
            && !self.invitations.has_pending()
            && self.contacts.is_empty()
            && self.homes.is_empty()
    }
}
