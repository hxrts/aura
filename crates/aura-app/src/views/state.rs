//! # Aggregate View State
//!
//! This module contains the aggregate view state that holds all view states.

use super::{
    ChatState, ContactsState, HomeState, HomesState, InvitationsState, NeighborhoodState,
    RecoveryState,
};
use crate::core::StateSnapshot;
#[cfg(feature = "signals")]
use aura_core::identifiers::ChannelId;
use cfg_if::cfg_if;

cfg_if! {
    if #[cfg(feature = "signals")] {
        use futures_signals::signal::{Mutable, Signal};
    }
}

cfg_if! {
    if #[cfg(feature = "signals")] {
        #[derive(Default)]
        /// Aggregate view state that holds all domain states.
        ///
        /// This is the main state container for the application.
        pub struct ViewState {
            /// Chat state (channels, messages)
            chat: Mutable<ChatState>,
            /// Recovery state (guardians, recovery status)
            recovery: Mutable<RecoveryState>,
            /// Invitations state
            invitations: Mutable<InvitationsState>,
            /// Contacts state
            contacts: Mutable<ContactsState>,
            /// Multi-home state (all homes the user has created/joined)
            homes: Mutable<HomesState>,
            /// Neighborhood state
            neighborhood: Mutable<NeighborhoodState>,
        }
    } else {
        #[derive(Default)]
        /// Aggregate view state that holds all domain states.
        ///
        /// This is the main state container for the application.
        pub struct ViewState {
            /// Chat state (channels, messages)
            chat: ChatState,
            /// Recovery state (guardians, recovery status)
            recovery: RecoveryState,
            /// Invitations state
            invitations: InvitationsState,
            /// Contacts state
            contacts: ContactsState,
            /// Multi-home state (all homes the user has created/joined)
            homes: HomesState,
            /// Neighborhood state
            neighborhood: NeighborhoodState,
        }
    }
}

impl ViewState {
    /// Get a snapshot of all view states
    pub fn snapshot(&self) -> StateSnapshot {
        cfg_if! {
            if #[cfg(feature = "signals")] {
                StateSnapshot {
                    chat: self.chat.get_cloned(),
                    recovery: self.recovery.get_cloned(),
                    invitations: self.invitations.get_cloned(),
                    contacts: self.contacts.get_cloned(),
                    homes: self.homes.get_cloned(),
                    neighborhood: self.neighborhood.get_cloned(),
                }
            } else {
                StateSnapshot {
                    chat: self.chat.clone(),
                    recovery: self.recovery.clone(),
                    invitations: self.invitations.clone(),
                    contacts: self.contacts.clone(),
                    homes: self.homes.clone(),
                    neighborhood: self.neighborhood.clone(),
                }
            }
        }
    }
}

// =============================================================================
// Signal-based API (native Rust, dominator)
// =============================================================================

cfg_if! {
    if #[cfg(feature = "signals")] {
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

            /// Get a clone of the current homes state
            ///
            /// This returns a snapshot of the homes state for read-only access.
            /// For reactive updates, use `homes_signal()` instead.
            pub fn get_homes(&self) -> HomesState {
                self.homes.get_cloned()
            }

            /// Get a clone of the current recovery state
            pub fn get_recovery(&self) -> RecoveryState {
                self.recovery.get_cloned()
            }

            /// Get a clone of the current invitations state
            pub fn get_invitations(&self) -> InvitationsState {
                self.invitations.get_cloned()
            }

            /// Get a clone of the current neighborhood state
            pub fn get_neighborhood(&self) -> NeighborhoodState {
                self.neighborhood.get_cloned()
            }

            /// Update chat state
            pub fn set_chat(&self, state: ChatState) {
                self.chat.set(state);
            }

            /// Select a channel (UI-only, not journaled)
            ///
            /// This updates the selected channel in ChatState and triggers
            /// the chat signal for UI updates.
            pub fn select_channel(&self, channel_id: Option<ChannelId>) {
                self.chat.lock_mut().select_channel(channel_id);
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

            /// Add a home to the homes state
            pub fn add_home(&self, home: HomeState) {
                self.homes.lock_mut().add_home(home);
            }

            /// Remove a home from the homes state
            pub fn remove_home(&self, home_id: &ChannelId) -> Option<HomeState> {
                self.homes.lock_mut().remove_home(home_id)
            }
        }
    }
}

// =============================================================================
// Non-signal API (UniFFI, callbacks)
// =============================================================================

cfg_if! {
    if #[cfg(not(feature = "signals"))] {
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

            /// Get a clone of the current neighborhood state
            ///
            /// This returns a snapshot of the neighborhood state for read-only access.
            pub fn get_neighborhood(&self) -> NeighborhoodState {
                self.neighborhood.clone()
            }

            /// Get a clone of the current homes state
            ///
            /// This returns a snapshot of the homes state for read-only access.
            pub fn get_homes(&self) -> HomesState {
                self.homes.clone()
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
    }
}
