//! # Aggregate View State
//!
//! This module contains the aggregate view state that holds all view states.

use super::{
    BlockState, BlocksState, ChatState, ContactsState, InvitationsState, NeighborhoodState,
    RecoveryState,
};
use crate::core::{StateSnapshot, ViewDelta};
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
            /// Current block state (for backwards compatibility)
            block: Mutable<BlockState>,
            /// Multi-block state (all blocks the user has created/joined)
            blocks: Mutable<BlocksState>,
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
            /// Current block state (for backwards compatibility)
            block: BlockState,
            /// Multi-block state (all blocks the user has created/joined)
            blocks: BlocksState,
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
                    block: self.block.get_cloned(),
                    blocks: self.blocks.get_cloned(),
                    neighborhood: self.neighborhood.get_cloned(),
                }
            } else {
                StateSnapshot {
                    chat: self.chat.clone(),
                    recovery: self.recovery.clone(),
                    invitations: self.invitations.clone(),
                    contacts: self.contacts.clone(),
                    block: self.block.clone(),
                    blocks: self.blocks.clone(),
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

            /// Get a signal for block state
            pub fn block_signal(&self) -> impl Signal<Item = BlockState> {
                self.block.signal_cloned()
            }

            /// Get a signal for neighborhood state
            pub fn neighborhood_signal(&self) -> impl Signal<Item = NeighborhoodState> {
                self.neighborhood.signal_cloned()
            }

            /// Get a signal for blocks state (multi-block management)
            pub fn blocks_signal(&self) -> impl Signal<Item = BlocksState> {
                self.blocks.signal_cloned()
            }

            /// Get a clone of the current blocks state
            ///
            /// This returns a snapshot of the blocks state for read-only access.
            /// For reactive updates, use `blocks_signal()` instead.
            pub fn get_blocks(&self) -> BlocksState {
                self.blocks.get_cloned()
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
            pub fn select_channel(&self, channel_id: Option<String>) {
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

            /// Update block state
            pub fn set_block(&self, state: BlockState) {
                self.block.set(state);
            }

            /// Update neighborhood state
            pub fn set_neighborhood(&self, state: NeighborhoodState) {
                self.neighborhood.set(state);
            }

            /// Update blocks state
            pub fn set_blocks(&self, state: BlocksState) {
                self.blocks.set(state);
            }

            /// Select a block (UI-only, not journaled)
            ///
            /// This updates the selected block in BlocksState and triggers
            /// the blocks signal for UI updates.
            pub fn select_block(&self, block_id: Option<String>) {
                self.blocks.lock_mut().select_block(block_id);
            }

            /// Add a block to the blocks state
            pub fn add_block(&self, block: BlockState) {
                self.blocks.lock_mut().add_block(block);
            }

            /// Remove a block from the blocks state
            pub fn remove_block(&self, block_id: &str) -> Option<BlockState> {
                self.blocks.lock_mut().remove_block(block_id)
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

            /// Get current block state
            pub fn block(&self) -> &BlockState {
                &self.block
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

            /// Get a clone of the current blocks state
            ///
            /// This returns a snapshot of the blocks state for read-only access.
            pub fn get_blocks(&self) -> BlocksState {
                self.blocks.clone()
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

            /// Update block state
            pub fn set_block(&mut self, state: BlockState) {
                self.block = state;
            }

            /// Update neighborhood state
            pub fn set_neighborhood(&mut self, state: NeighborhoodState) {
                self.neighborhood = state;
            }

            /// Get current blocks state
            pub fn blocks(&self) -> &BlocksState {
                &self.blocks
            }

            /// Get mutable blocks state
            pub fn blocks_mut(&mut self) -> &mut BlocksState {
                &mut self.blocks
            }

            /// Update blocks state
            pub fn set_blocks(&mut self, state: BlocksState) {
                self.blocks = state;
            }
        }
    }
}

// =============================================================================
// View Delta Application
// =============================================================================

cfg_if! {
    if #[cfg(feature = "signals")] {
        impl ViewState {
            /// Apply a view delta to update the appropriate state
            ///
            /// This is called after a journal fact is reduced to update the view.
            pub fn apply_delta(&self, delta: ViewDelta) {
                match delta {
                    ViewDelta::MessageSent { channel_id, mut message } => {
                        // Resolve sender_name from contacts if available
                        let contacts = self.contacts.lock_ref();
                        message.sender_name = contacts.get_display_name(&message.sender_id);
                        drop(contacts);
                        self.chat.lock_mut().apply_message(channel_id, message);
                    }
                    ViewDelta::ChannelCreated { channel } => {
                        self.chat.lock_mut().add_channel(channel);
                    }
                    ViewDelta::ChannelJoined { channel_id } => {
                        // Mark channel as joined (increment member count)
                        self.chat.lock_mut().mark_channel_joined(&channel_id);
                    }
                    ViewDelta::ChannelLeft { channel_id } => {
                        self.chat.lock_mut().remove_channel(&channel_id);
                    }
                    ViewDelta::ChannelClosed { channel_id } => {
                        self.chat.lock_mut().remove_channel(&channel_id);
                    }
                    ViewDelta::TopicUpdated { channel_id, topic } => {
                        self.chat.lock_mut().update_topic(&channel_id, topic);
                    }
                    ViewDelta::NicknameSet { target, nickname } => {
                        self.contacts.lock_mut().set_nickname(target, nickname);
                    }
                    ViewDelta::BlockNameSet { name, .. } => {
                        self.block.lock_mut().set_name(name);
                    }
                    ViewDelta::RecoveryRequested { session_id } => {
                        // Use empty account_id and 0 as initiated_at since we don't have them in delta
                        self.recovery.lock_mut().initiate_recovery(session_id, String::new(), 0);
                    }
                    ViewDelta::GuardianApproved { guardian_id } => {
                        self.recovery.lock_mut().add_guardian_approval(guardian_id);
                    }
                    ViewDelta::InvitationCreated { invitation_id } => {
                        // Create a minimal invitation record - full details would come from reducer
                        use super::invitations::{Invitation, InvitationDirection, InvitationStatus, InvitationType};
                        let invitation = Invitation {
                            id: invitation_id,
                            invitation_type: InvitationType::Block,
                            status: InvitationStatus::Pending,
                            direction: InvitationDirection::Sent,
                            from_id: String::new(),
                            from_name: String::new(),
                            to_id: None,
                            to_name: None,
                            created_at: 0,
                            expires_at: None,
                            message: None,
                            block_id: None,
                            block_name: None,
                        };
                        self.invitations.lock_mut().add_invitation(invitation);
                    }
                    ViewDelta::InvitationAccepted { invitation_id } => {
                        self.invitations.lock_mut().accept_invitation(&invitation_id);
                    }
                    ViewDelta::InvitationRejected { invitation_id } => {
                        self.invitations.lock_mut().reject_invitation(&invitation_id);
                    }
                    ViewDelta::GuardianToggled {
                        contact_id,
                        is_guardian,
                    } => {
                        self.recovery.lock_mut().toggle_guardian(contact_id, is_guardian);
                    }
                    ViewDelta::GuardianThresholdSet { threshold } => {
                        self.recovery.lock_mut().set_threshold(threshold);
                    }
                    ViewDelta::Unknown { .. } => {
                        // Unknown deltas are ignored
                    }
                }
            }
        }
    } else {
        impl ViewState {
            /// Apply a view delta to update the appropriate state
            ///
            /// This is called after a journal fact is reduced to update the view.
            pub fn apply_delta(&mut self, delta: ViewDelta) {
                match delta {
                    ViewDelta::MessageSent { channel_id, mut message } => {
                        // Resolve sender_name from contacts if available
                        message.sender_name = self.contacts.get_display_name(&message.sender_id);
                        self.chat.apply_message(channel_id, message);
                    }
                    ViewDelta::ChannelCreated { channel } => {
                        self.chat.add_channel(channel);
                    }
                    ViewDelta::ChannelJoined { channel_id } => {
                        self.chat.mark_channel_joined(&channel_id);
                    }
                    ViewDelta::ChannelLeft { channel_id } => {
                        self.chat.remove_channel(&channel_id);
                    }
                    ViewDelta::ChannelClosed { channel_id } => {
                        self.chat.remove_channel(&channel_id);
                    }
                    ViewDelta::TopicUpdated { channel_id, topic } => {
                        self.chat.update_topic(&channel_id, topic);
                    }
                    ViewDelta::NicknameSet { target, nickname } => {
                        self.contacts.set_nickname(target, nickname);
                    }
                    ViewDelta::BlockNameSet { name, .. } => {
                        self.block.set_name(name);
                    }
                    ViewDelta::RecoveryRequested { session_id } => {
                        // Use empty account_id and 0 as initiated_at since we don't have them in delta
                        self.recovery.initiate_recovery(session_id, String::new(), 0);
                    }
                    ViewDelta::GuardianApproved { guardian_id } => {
                        self.recovery.add_guardian_approval(guardian_id);
                    }
                    ViewDelta::InvitationCreated { invitation_id } => {
                        // Create a minimal invitation record - full details would come from reducer
                        use super::invitations::{Invitation, InvitationDirection, InvitationStatus, InvitationType};
                        let invitation = Invitation {
                            id: invitation_id,
                            invitation_type: InvitationType::Block,
                            status: InvitationStatus::Pending,
                            direction: InvitationDirection::Sent,
                            from_id: String::new(),
                            from_name: String::new(),
                            to_id: None,
                            to_name: None,
                            created_at: 0,
                            expires_at: None,
                            message: None,
                            block_id: None,
                            block_name: None,
                        };
                        self.invitations.add_invitation(invitation);
                    }
                    ViewDelta::InvitationAccepted { invitation_id } => {
                        self.invitations.accept_invitation(&invitation_id);
                    }
                    ViewDelta::InvitationRejected { invitation_id } => {
                        self.invitations.reject_invitation(&invitation_id);
                    }
                    ViewDelta::GuardianToggled {
                        contact_id,
                        is_guardian,
                    } => {
                        self.recovery.toggle_guardian(contact_id, is_guardian);
                    }
                    ViewDelta::GuardianThresholdSet { threshold } => {
                        self.recovery.set_threshold(threshold);
                    }
                    ViewDelta::Unknown { .. } => {
                        // Unknown deltas are ignored
                    }
                }
            }
        }
    }
}
