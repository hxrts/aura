//! # Aggregate View State
//!
//! This module contains the aggregate view state that holds all view states.

use super::{
    ChatState, ContactsState, HomesState, InvitationsState, NeighborhoodState, RecoveryState,
};
use crate::core::StateSnapshot;
#[cfg(feature = "signals")]
use aura_core::types::identifiers::ChannelId;
#[cfg(feature = "signals")]
use futures_signals::signal::{Mutable, Signal};

#[cfg(feature = "signals")]
type ViewCell<T> = Mutable<T>;
#[cfg(not(feature = "signals"))]
type ViewCell<T> = T;

#[cfg(feature = "signals")]
fn clone_view_cell<T: Clone>(cell: &Mutable<T>) -> T {
    cell.get_cloned()
}

#[cfg(not(feature = "signals"))]
fn clone_view_cell<T: Clone>(cell: &T) -> T {
    cell.clone()
}

#[derive(Default)]
/// Aggregate view state that holds all domain states.
///
/// This is the main state container for the application.
pub struct ViewState {
    /// Chat state (channels, messages)
    chat: ViewCell<ChatState>,
    /// Recovery state (guardians, recovery status)
    recovery: ViewCell<RecoveryState>,
    /// Invitations state
    invitations: ViewCell<InvitationsState>,
    /// Contacts state
    contacts: ViewCell<ContactsState>,
    /// Multi-home state (all homes the user has created/joined)
    homes: ViewCell<HomesState>,
    /// Neighborhood state
    neighborhood: ViewCell<NeighborhoodState>,
}

impl ViewState {
    /// Get a snapshot of all view states
    pub fn snapshot(&self) -> StateSnapshot {
        StateSnapshot {
            chat: clone_view_cell(&self.chat),
            recovery: clone_view_cell(&self.recovery),
            invitations: clone_view_cell(&self.invitations),
            contacts: clone_view_cell(&self.contacts),
            homes: clone_view_cell(&self.homes),
            neighborhood: clone_view_cell(&self.neighborhood),
        }
    }

    /// Get a clone of the current chat state.
    pub fn get_chat(&self) -> ChatState {
        clone_view_cell(&self.chat)
    }

    /// Get a clone of the current recovery state.
    pub fn get_recovery(&self) -> RecoveryState {
        clone_view_cell(&self.recovery)
    }

    /// Get a clone of the current invitations state.
    pub fn get_invitations(&self) -> InvitationsState {
        clone_view_cell(&self.invitations)
    }

    /// Get a clone of the current contacts state.
    pub fn get_contacts(&self) -> ContactsState {
        clone_view_cell(&self.contacts)
    }

    /// Get a clone of the current neighborhood state.
    pub fn get_neighborhood(&self) -> NeighborhoodState {
        clone_view_cell(&self.neighborhood)
    }

    /// Get a clone of the current homes state.
    ///
    /// This returns a snapshot of the homes state for read-only access.
    pub fn get_homes(&self) -> HomesState {
        clone_view_cell(&self.homes)
    }
}

// =============================================================================
// Signal-based API (native Rust, dominator)
// =============================================================================

#[cfg(feature = "signals")]
impl ViewState {
    /// Get a signal for chat state
    pub fn chat_signal(&self) -> impl Signal<Item = ChatState> {
        self.chat.signal_cloned()
    }

    /// Get a signal for recovery state
    pub fn recovery_signal(&self) -> impl Signal<Item = RecoveryState> {
        self.recovery.signal_cloned()
    }

    /// Get a signal for invitations state
    pub fn invitations_signal(&self) -> impl Signal<Item = InvitationsState> {
        self.invitations.signal_cloned()
    }

    /// Get a signal for contacts state
    pub fn contacts_signal(&self) -> impl Signal<Item = ContactsState> {
        self.contacts.signal_cloned()
    }

    /// Get a signal for neighborhood state
    pub fn neighborhood_signal(&self) -> impl Signal<Item = NeighborhoodState> {
        self.neighborhood.signal_cloned()
    }

    /// Get a signal for homes state (multi-home management)
    pub fn homes_signal(&self) -> impl Signal<Item = HomesState> {
        self.homes.signal_cloned()
    }

    /// Update chat state
    pub fn set_chat(&self, state: ChatState) {
        self.chat.set(state);
    }

    /// Update recovery state
    pub fn set_recovery(&self, state: RecoveryState) {
        self.recovery.set(state);
    }

    /// Update invitations state
    pub fn set_invitations(&self, state: InvitationsState) {
        self.invitations.set(state);
    }

    /// Update contacts state
    pub fn set_contacts(&self, state: ContactsState) {
        self.contacts.set(state);
    }

    /// Update neighborhood state
    pub fn set_neighborhood(&self, state: NeighborhoodState) {
        self.neighborhood.set(state);
    }

    /// Update homes state
    pub fn set_homes(&self, state: HomesState) {
        self.homes.set(state);
    }

    /// Select a home (UI-only, not journaled)
    ///
    /// This updates the selected home in HomesState and triggers
    /// the homes signal for UI updates.
    pub fn select_home(&self, home_id: Option<ChannelId>) {
        self.homes.lock_mut().select_home(home_id);
    }
}

// =============================================================================
// Non-signal API (UniFFI, callbacks)
// =============================================================================

#[cfg(not(feature = "signals"))]
impl ViewState {
    /// Get current chat state
    pub fn chat(&self) -> &ChatState {
        &self.chat
    }

    /// Get current recovery state
    pub fn recovery(&self) -> &RecoveryState {
        &self.recovery
    }

    /// Get current invitations state
    pub fn invitations(&self) -> &InvitationsState {
        &self.invitations
    }

    /// Get current contacts state
    pub fn contacts(&self) -> &ContactsState {
        &self.contacts
    }

    /// Get current neighborhood state
    pub fn neighborhood(&self) -> &NeighborhoodState {
        &self.neighborhood
    }

    /// Update chat state
    pub fn set_chat(&mut self, state: ChatState) {
        self.chat = state;
    }

    /// Update recovery state
    pub fn set_recovery(&mut self, state: RecoveryState) {
        self.recovery = state;
    }

    /// Update invitations state
    pub fn set_invitations(&mut self, state: InvitationsState) {
        self.invitations = state;
    }

    /// Update contacts state
    pub fn set_contacts(&mut self, state: ContactsState) {
        self.contacts = state;
    }

    /// Update neighborhood state
    pub fn set_neighborhood(&mut self, state: NeighborhoodState) {
        self.neighborhood = state;
    }

    /// Get current homes state
    pub fn homes(&self) -> &HomesState {
        &self.homes
    }

    /// Get mutable homes state
    pub fn homes_mut(&mut self) -> &mut HomesState {
        &mut self.homes
    }

    /// Update homes state
    pub fn set_homes(&mut self, state: HomesState) {
        self.homes = state;
    }
}
