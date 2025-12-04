//! # iocraft Context
//!
//! Self-contained context for iocraft TUI components.
//!
//! ## Usage
//!
//! ```ignore
//! use crate::tui::context::IoContext;
//!
//! let ctx = IoContext::new(bridge);
//!
//! // Get reactive data snapshots
//! let chat = ctx.snapshot_chat();
//! let guardians = ctx.snapshot_guardians();
//!
//! // Get iocraft-compatible data for screens
//! let channels = ctx.get_channels();
//! let messages = ctx.get_messages();
//!
//! // Dispatch effects
//! ctx.dispatch_send_message("channel-1", "Hello!").await;
//! ```

use std::sync::Arc;

use crate::tui::effects::{AuraEvent, EffectBridge, EffectCommand, EventFilter, EventSubscription};
use crate::tui::hooks::{
    snapshot_block, snapshot_chat, snapshot_contacts, snapshot_guardians, snapshot_invitations,
    snapshot_neighborhood, snapshot_recovery, BlockSnapshot, ChatSnapshot, ContactsSnapshot,
    GuardiansSnapshot, InvitationsSnapshot, NeighborhoodSnapshot, RecoverySnapshot,
};
use crate::tui::reactive::views::{
    BlockView, ChatView, ContactsView, GuardiansView, InvitationsView, NeighborhoodView,
    RecoveryView,
};
use crate::tui::types::{
    BlockBudget, Channel, Contact, Guardian, Invitation, Message, RecoveryStatus, Resident,
};

/// iocraft-friendly context
///
/// Self-contained context providing snapshot-based access to reactive views
/// and effect dispatch for iocraft components.
#[derive(Clone)]
pub struct IoContext {
    /// Effect bridge for command dispatch
    bridge: Arc<EffectBridge>,

    /// Reactive views
    chat_view: Arc<ChatView>,
    guardians_view: Arc<GuardiansView>,
    recovery_view: Arc<RecoveryView>,
    invitations_view: Arc<InvitationsView>,
    block_view: Arc<BlockView>,
    contacts_view: Arc<ContactsView>,
    neighborhood_view: Arc<NeighborhoodView>,
}

impl IoContext {
    /// Create a new IoContext with an effect bridge
    pub fn new(bridge: EffectBridge) -> Self {
        Self {
            bridge: Arc::new(bridge),
            chat_view: Arc::new(ChatView::new()),
            guardians_view: Arc::new(GuardiansView::new()),
            recovery_view: Arc::new(RecoveryView::new()),
            invitations_view: Arc::new(InvitationsView::new()),
            block_view: Arc::new(BlockView::new()),
            contacts_view: Arc::new(ContactsView::new()),
            neighborhood_view: Arc::new(NeighborhoodView::new()),
        }
    }

    /// Create with default bridge configuration
    pub fn with_defaults() -> Self {
        Self::new(EffectBridge::new())
    }

    // ─── View Accessors ─────────────────────────────────────────────────────

    /// Get the chat view
    pub fn chat_view(&self) -> &ChatView {
        &self.chat_view
    }

    /// Get the guardians view
    pub fn guardians_view(&self) -> &GuardiansView {
        &self.guardians_view
    }

    /// Get the recovery view
    pub fn recovery_view(&self) -> &RecoveryView {
        &self.recovery_view
    }

    /// Get the invitations view
    pub fn invitations_view(&self) -> &InvitationsView {
        &self.invitations_view
    }

    /// Get the block view
    pub fn block_view(&self) -> &BlockView {
        &self.block_view
    }

    /// Get the contacts view
    pub fn contacts_view(&self) -> &ContactsView {
        &self.contacts_view
    }

    /// Get the neighborhood view
    pub fn neighborhood_view(&self) -> &NeighborhoodView {
        &self.neighborhood_view
    }

    // ─── Snapshot Accessors ─────────────────────────────────────────────────

    /// Get a snapshot of chat data (channels and messages)
    pub fn snapshot_chat(&self) -> ChatSnapshot {
        snapshot_chat(&self.chat_view)
    }

    /// Get a snapshot of guardians data
    pub fn snapshot_guardians(&self) -> GuardiansSnapshot {
        snapshot_guardians(&self.guardians_view)
    }

    /// Get a snapshot of recovery data
    pub fn snapshot_recovery(&self) -> RecoverySnapshot {
        snapshot_recovery(&self.recovery_view)
    }

    /// Get a snapshot of invitations data
    pub fn snapshot_invitations(&self) -> InvitationsSnapshot {
        snapshot_invitations(&self.invitations_view)
    }

    /// Get a snapshot of block data
    pub fn snapshot_block(&self) -> BlockSnapshot {
        snapshot_block(&self.block_view)
    }

    /// Get a snapshot of contacts data
    pub fn snapshot_contacts(&self) -> ContactsSnapshot {
        snapshot_contacts(&self.contacts_view)
    }

    /// Get a snapshot of neighborhood data
    pub fn snapshot_neighborhood(&self) -> NeighborhoodSnapshot {
        snapshot_neighborhood(&self.neighborhood_view)
    }

    // ─── iocraft-Compatible Getters ────────────────────────────────────────

    /// Get channels as iocraft types for ChatScreen
    pub fn get_channels(&self) -> Vec<Channel> {
        let chat = self.snapshot_chat();
        let selected_id = chat.selected_channel.as_deref();
        chat.channels
            .iter()
            .map(|c| Channel::from_app(c, selected_id == Some(c.id.as_str())))
            .collect()
    }

    /// Get messages for current channel as iocraft types
    pub fn get_messages(&self) -> Vec<Message> {
        let chat = self.snapshot_chat();
        chat.messages.iter().map(|m| m.into()).collect()
    }

    /// Get selected channel ID
    pub fn get_selected_channel(&self) -> Option<String> {
        self.snapshot_chat().selected_channel
    }

    /// Get guardians as iocraft types
    pub fn get_guardians(&self) -> Vec<Guardian> {
        let snap = self.snapshot_guardians();
        snap.guardians.iter().map(|g| g.into()).collect()
    }

    /// Get recovery status as iocraft type
    pub fn get_recovery_status(&self) -> RecoveryStatus {
        let snap = self.snapshot_recovery();
        (&snap.status).into()
    }

    /// Get invitations as iocraft types
    pub fn get_invitations(&self) -> Vec<Invitation> {
        let snap = self.snapshot_invitations();
        snap.invitations.iter().map(|i| i.into()).collect()
    }

    /// Get contacts as iocraft types
    pub fn get_contacts(&self) -> Vec<Contact> {
        let snap = self.snapshot_contacts();
        snap.contacts.iter().map(|c| c.into()).collect()
    }

    /// Get block residents as iocraft types
    pub fn get_residents(&self) -> Vec<Resident> {
        let snap = self.snapshot_block();
        snap.residents.iter().map(|r| r.into()).collect()
    }

    /// Get block budget as iocraft type
    pub fn get_block_budget(&self) -> BlockBudget {
        let snap = self.snapshot_block();
        let mut budget: BlockBudget = (&snap.storage).into();
        budget.resident_count = snap.residents.len() as u8;
        budget
    }

    // ─── Effect Dispatch ────────────────────────────────────────────────────

    /// Dispatch a command (fire and forget)
    pub async fn dispatch(&self, command: EffectCommand) -> Result<(), String> {
        self.bridge.dispatch(command).await
    }

    /// Dispatch a command and wait for completion
    pub async fn dispatch_and_wait(&self, command: EffectCommand) -> Result<(), String> {
        self.bridge.dispatch_and_wait(command).await
    }

    /// Send a message to a channel
    pub async fn dispatch_send_message(
        &self,
        channel_id: &str,
        content: &str,
    ) -> Result<(), String> {
        self.dispatch(EffectCommand::SendMessage {
            channel: channel_id.to_string(),
            content: content.to_string(),
        })
        .await
    }

    /// Join a channel
    pub async fn dispatch_join_channel(&self, channel_id: &str) -> Result<(), String> {
        self.dispatch(EffectCommand::JoinChannel {
            channel: channel_id.to_string(),
        })
        .await
    }

    /// Leave a channel
    pub async fn dispatch_leave_channel(&self, channel_id: &str) -> Result<(), String> {
        self.dispatch(EffectCommand::LeaveChannel {
            channel: channel_id.to_string(),
        })
        .await
    }

    /// Start recovery
    pub async fn dispatch_start_recovery(&self) -> Result<(), String> {
        self.dispatch(EffectCommand::StartRecovery).await
    }

    /// Submit guardian approval for recovery
    pub async fn dispatch_submit_guardian_approval(&self, guardian_id: &str) -> Result<(), String> {
        self.dispatch(EffectCommand::SubmitGuardianApproval {
            guardian_id: guardian_id.to_string(),
        })
        .await
    }

    // ─── Event Subscription ─────────────────────────────────────────────────

    /// Subscribe to events with a filter
    pub fn subscribe(&self, filter: EventFilter) -> EventSubscription {
        self.bridge.subscribe(filter)
    }

    /// Subscribe to all events
    pub fn subscribe_all(&self) -> EventSubscription {
        self.bridge.subscribe_all()
    }

    /// Emit an event (for testing)
    pub fn emit(&self, event: AuraEvent) {
        self.bridge.emit(event);
    }

    // ─── Connection Status ──────────────────────────────────────────────────

    /// Check if connected to the effect system
    pub async fn is_connected(&self) -> bool {
        self.bridge.is_connected().await
    }

    /// Get last error if any
    pub async fn last_error(&self) -> Option<String> {
        self.bridge.last_error().await
    }
}

impl Default for IoContext {
    fn default() -> Self {
        Self::with_defaults()
    }
}

/// Trait for iocraft props that need context access
///
/// Implement this to enable context injection into components.
pub trait HasContext {
    /// Set the context reference
    fn set_context(&mut self, ctx: Arc<IoContext>);

    /// Get the context reference
    fn context(&self) -> Option<&Arc<IoContext>>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_io_context_creation() {
        let ctx = IoContext::with_defaults();

        // Verify snapshots work
        let chat = ctx.snapshot_chat();
        assert!(chat.channels.is_empty());

        let guardians = ctx.snapshot_guardians();
        assert!(guardians.guardians.is_empty());
    }

    #[tokio::test]
    async fn test_io_context_dispatch() {
        let ctx = IoContext::with_defaults();

        // Dispatch should succeed (command is processed)
        let result = ctx.dispatch(EffectCommand::Ping).await;
        assert!(result.is_ok());
    }
}
