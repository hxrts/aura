//! # TUI Context
//!
//! Shared context for TUI screens, providing access to the effect bridge,
//! query executor, and reactive views.

use std::sync::Arc;
use tokio::sync::RwLock;

use super::demo::{Tip, TipContext, TipProvider};
use super::effects::{AuraEvent, EffectBridge, EffectCommand, EventFilter, EventSubscription};
use super::reactive::{
    executor::{DataUpdate, QueryExecutor},
    views::{ChatView, GuardiansView, InvitationsView, RecoveryView},
    RecoveryState,
};
use super::screens::ScreenType;

/// Shared TUI context
///
/// This provides access to the effect bridge, query executor, and reactive views.
#[derive(Clone)]
pub struct TuiContext {
    /// Effect bridge for command dispatch and event subscription
    bridge: Arc<EffectBridge>,

    /// Query executor for reactive data queries
    query_executor: Arc<QueryExecutor>,

    /// Reactive views
    chat_view: Arc<ChatView>,
    guardians_view: Arc<GuardiansView>,
    recovery_view: Arc<RecoveryView>,
    invitations_view: Arc<InvitationsView>,

    /// Current user's authority ID (set after onboarding)
    authority_id: Arc<RwLock<Option<aura_core::AuthorityId>>>,

    /// Optional tip provider for demo mode
    tip_provider: Option<Arc<RwLock<Box<dyn TipProvider>>>>,

    /// Track whether user has sent a message (for tip context)
    has_sent_message: Arc<RwLock<bool>>,
}

impl TuiContext {
    /// Create a new TUI context with an effect bridge
    pub fn new(bridge: EffectBridge) -> Self {
        let query_executor = Arc::new(QueryExecutor::new());
        let chat_view = Arc::new(ChatView::new());
        let guardians_view = Arc::new(GuardiansView::new());
        let recovery_view = Arc::new(RecoveryView::new());
        let invitations_view = Arc::new(InvitationsView::new());

        let ctx = Self {
            bridge: Arc::new(bridge),
            query_executor: query_executor.clone(),
            chat_view: chat_view.clone(),
            guardians_view: guardians_view.clone(),
            recovery_view: recovery_view.clone(),
            invitations_view: invitations_view.clone(),
            authority_id: Arc::new(RwLock::new(None)),
            tip_provider: None,
            has_sent_message: Arc::new(RwLock::new(false)),
        };

        // Start background tasks to sync query executor data to views
        ctx.start_view_sync_tasks();

        ctx
    }

    /// Create a new TUI context with default bridge configuration
    pub fn with_defaults() -> Self {
        Self::new(EffectBridge::new())
    }

    /// Create a new TUI context for demo mode with a tip provider
    pub fn with_demo(bridge: EffectBridge, tip_provider: impl TipProvider + 'static) -> Self {
        let query_executor = Arc::new(QueryExecutor::new());
        let chat_view = Arc::new(ChatView::new());
        let guardians_view = Arc::new(GuardiansView::new());
        let recovery_view = Arc::new(RecoveryView::new());
        let invitations_view = Arc::new(InvitationsView::new());

        let ctx = Self {
            bridge: Arc::new(bridge),
            query_executor: query_executor.clone(),
            chat_view: chat_view.clone(),
            guardians_view: guardians_view.clone(),
            recovery_view: recovery_view.clone(),
            invitations_view: invitations_view.clone(),
            authority_id: Arc::new(RwLock::new(None)),
            tip_provider: Some(Arc::new(RwLock::new(Box::new(tip_provider)))),
            has_sent_message: Arc::new(RwLock::new(false)),
        };

        // Start background tasks to sync query executor data to views
        ctx.start_view_sync_tasks();

        ctx
    }

    /// Start background tasks to sync data from QueryExecutor to Views
    fn start_view_sync_tasks(&self) {
        // Start query executor sync loop
        let ctx = self.clone();
        tokio::spawn(async move {
            ctx.sync_views_loop().await;
        });

        // Start bridge event listener loop
        let ctx2 = self.clone();
        tokio::spawn(async move {
            ctx2.bridge_events_loop().await;
        });
    }

    /// Background loop to process bridge events and update views
    async fn bridge_events_loop(&self) {
        let mut event_sub = self.bridge.subscribe(EventFilter::all());

        loop {
            match event_sub.recv().await {
                Some(event) => {
                    self.handle_bridge_event(event).await;
                }
                None => {
                    // Channel closed, exit
                    break;
                }
            }
        }
    }

    /// Handle a bridge event and update appropriate views
    async fn handle_bridge_event(&self, event: AuraEvent) {
        use super::reactive::queries::Message;

        match event {
            AuraEvent::MessageReceived {
                channel,
                from,
                content,
                timestamp,
            } => {
                let message = Message {
                    id: format!("msg-{}-{}", channel, timestamp),
                    channel_id: channel.clone(),
                    sender_id: from.clone(),
                    sender_name: from,
                    content,
                    timestamp,
                    read: true,
                    is_own: true, // Message we just sent
                    reply_to: None,
                };
                self.chat_view.add_message(&channel, message).await;
            }
            AuraEvent::UserJoined { channel, user } => {
                tracing::info!("User {} joined channel {}", user, channel);
            }
            AuraEvent::UserLeft { channel, user } => {
                tracing::info!("User {} left channel {}", user, channel);
            }
            AuraEvent::RecoveryStarted { session_id } => {
                tracing::info!("Recovery started: {}", session_id);
            }
            AuraEvent::GuardianApproved {
                guardian_id,
                current,
                threshold,
            } => {
                tracing::info!(
                    "Guardian {} approved ({}/{})",
                    guardian_id,
                    current,
                    threshold
                );
                self.recovery_view.record_approval(guardian_id).await;
            }
            AuraEvent::RecoveryCompleted { session_id } => {
                tracing::info!("Recovery completed: {}", session_id);
            }
            AuraEvent::Error { code, message } => {
                tracing::error!("Bridge error {}: {}", code, message);
            }
            _ => {
                // Other events logged but not specially handled
                tracing::debug!("Bridge event: {:?}", event);
            }
        }
    }

    /// Background loop to sync query executor data to views
    async fn sync_views_loop(&self) {
        let mut update_rx = self.query_executor.subscribe();

        loop {
            tokio::select! {
                Ok(update) = update_rx.recv() => {
                    match update {
                        DataUpdate::ChannelsUpdated => {
                            if let Ok(channels) = self.query_executor.execute_channels_query(
                                &super::reactive::ChannelsQuery::new()
                            ).await {
                                self.chat_view.update_channels(channels).await;
                            }
                        }
                        DataUpdate::MessagesUpdated { channel_id } => {
                            if let Ok(messages) = self.query_executor.execute_messages_query(
                                &super::reactive::MessagesQuery::new(channel_id.clone())
                            ).await {
                                self.chat_view.update_messages(&channel_id, messages).await;
                            }
                        }
                        DataUpdate::GuardiansUpdated => {
                            if let Ok(guardians) = self.query_executor.execute_guardians_query(
                                &super::reactive::GuardiansQuery::new()
                            ).await {
                                self.guardians_view.update_guardians(guardians).await;
                            }
                        }
                        DataUpdate::RecoveryUpdated => {
                            if let Ok(recovery_status) = self.query_executor.execute_recovery_query(
                                &super::reactive::RecoveryQuery::active()
                            ).await {
                                self.recovery_view.update_status(recovery_status).await;
                            }
                        }
                        DataUpdate::InvitationsUpdated => {
                            if let Ok(invitations) = self.query_executor.execute_invitations_query(
                                &super::reactive::InvitationsQuery::new()
                            ).await {
                                self.invitations_view.update_invitations(invitations).await;
                            }
                        }
                    }
                }
                else => {
                    // Channel closed, exit
                    break;
                }
            }
        }
    }

    /// Get a reference to the effect bridge
    pub fn bridge(&self) -> &EffectBridge {
        &self.bridge
    }

    /// Dispatch a command through the effect bridge
    pub async fn dispatch(&self, command: EffectCommand) -> Result<(), String> {
        self.bridge.dispatch(command).await
    }

    /// Dispatch a command and wait for completion
    pub async fn dispatch_and_wait(&self, command: EffectCommand) -> Result<(), String> {
        self.bridge.dispatch_and_wait(command).await
    }

    /// Subscribe to events with a filter
    pub fn subscribe(&self, filter: EventFilter) -> EventSubscription {
        self.bridge.subscribe(filter)
    }

    /// Subscribe to all events
    pub fn subscribe_all(&self) -> EventSubscription {
        self.bridge.subscribe_all()
    }

    /// Emit an event (for testing or simulation)
    pub fn emit(&self, event: AuraEvent) {
        self.bridge.emit(event);
    }

    /// Set the current user's authority ID
    pub async fn set_authority(&self, authority_id: aura_core::AuthorityId) {
        let mut guard = self.authority_id.write().await;
        *guard = Some(authority_id);
    }

    /// Get the current user's authority ID
    pub async fn authority(&self) -> Option<aura_core::AuthorityId> {
        *self.authority_id.read().await
    }

    /// Check if connected
    pub async fn is_connected(&self) -> bool {
        self.bridge.is_connected().await
    }

    /// Get last error
    pub async fn last_error(&self) -> Option<String> {
        self.bridge.last_error().await
    }

    /// Get a reference to the query executor
    pub fn query_executor(&self) -> &QueryExecutor {
        &self.query_executor
    }

    /// Get a reference to the chat view
    pub fn chat_view(&self) -> &ChatView {
        &self.chat_view
    }

    /// Get a reference to the guardians view
    pub fn guardians_view(&self) -> &GuardiansView {
        &self.guardians_view
    }

    /// Get a reference to the recovery view
    pub fn recovery_view(&self) -> &RecoveryView {
        &self.recovery_view
    }

    /// Get a reference to the invitations view
    pub fn invitations_view(&self) -> &InvitationsView {
        &self.invitations_view
    }

    // ─── Tip System Methods ─────────────────────────────────────────────────

    /// Check if tips are enabled (demo mode active)
    pub fn has_tip_provider(&self) -> bool {
        self.tip_provider.is_some()
    }

    /// Get the current tip for a given screen
    pub async fn current_tip(&self, screen: ScreenType) -> Option<Tip> {
        let provider = self.tip_provider.as_ref()?;
        let context = self.build_tip_context(screen).await;
        let guard = provider.read().await;
        guard.current_tip(&context)
    }

    /// Build tip context from current state
    async fn build_tip_context(&self, screen: ScreenType) -> TipContext {
        let recovery_status = self.recovery_view.cached_status();
        let has_sent_message = *self.has_sent_message.read().await;

        let (recovery_active, approvals, threshold) = if let Some(ref status) = recovery_status {
            match status.state {
                RecoveryState::Initiated | RecoveryState::InProgress => (
                    true,
                    status.approvals_received as u8,
                    status.threshold as u8,
                ),
                RecoveryState::ThresholdMet => (
                    true,
                    status.approvals_received as u8,
                    status.threshold as u8,
                ),
                _ => (false, 0, status.threshold as u8),
            }
        } else {
            (false, 0, 2) // Default threshold
        };

        let mut ctx = TipContext::new(screen).with_recovery(recovery_active, approvals, threshold);
        ctx.has_sent_message = has_sent_message;
        ctx
    }

    /// Mark that the user has sent a message
    pub async fn mark_message_sent(&self) {
        let mut guard = self.has_sent_message.write().await;
        *guard = true;
    }

    /// Dismiss a tip by ID
    pub async fn dismiss_tip(&self, tip_id: &str) {
        if let Some(provider) = &self.tip_provider {
            let mut guard = provider.write().await;
            guard.dismiss_tip(tip_id);
        }
    }

    /// Enable or disable tips
    pub async fn set_tips_enabled(&self, enabled: bool) {
        if let Some(provider) = &self.tip_provider {
            let mut guard = provider.write().await;
            guard.set_tips_enabled(enabled);
        }
    }

    /// Check if tips are enabled
    pub async fn tips_enabled(&self) -> bool {
        if let Some(provider) = &self.tip_provider {
            let guard = provider.read().await;
            guard.tips_enabled()
        } else {
            false
        }
    }

    // ─── Demo Data Methods ─────────────────────────────────────────────────

    /// Load demo data into the views
    ///
    /// This populates the reactive views with sample data for demo mode.
    /// Call this after creating the context with `with_demo()`.
    pub async fn load_demo_data(&self) {
        use super::demo::MockStore;

        let store = MockStore::new();
        store.load_demo_data().await;

        // Populate chat view
        let channels = store.get_channels().await;
        self.chat_view.update_channels(channels).await;

        // Load messages for each channel
        for channel_id in ["general", "guardians"] {
            let messages = store.get_messages(channel_id).await;
            self.chat_view.update_messages(channel_id, messages).await;
        }

        // Populate guardians view
        let guardians = store.get_guardians().await;
        self.guardians_view.update_guardians(guardians).await;
        self.guardians_view.update_threshold(2, 2).await;

        // Populate invitations view
        let invitations = store.get_invitations().await;
        self.invitations_view.update_invitations(invitations).await;
    }
}

impl Default for TuiContext {
    fn default() -> Self {
        Self::with_defaults()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_context_creation() {
        let ctx = TuiContext::with_defaults();
        assert!(!ctx.is_connected().await);
        assert!(ctx.authority().await.is_none());
    }

    #[tokio::test]
    async fn test_authority_management() {
        let ctx = TuiContext::with_defaults();
        let auth_id = crate::ids::authority_id("tui:test-authority");

        ctx.set_authority(auth_id).await;
        assert_eq!(ctx.authority().await, Some(auth_id));
    }

    #[tokio::test]
    async fn test_command_dispatch() {
        let ctx = TuiContext::with_defaults();
        let result = ctx.dispatch(EffectCommand::Ping).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_event_subscription() {
        let ctx = TuiContext::with_defaults();
        let mut sub = ctx.subscribe(EventFilter::all());

        // Emit an event
        ctx.emit(AuraEvent::Connected);

        // Should receive the event
        let event = sub.try_recv();
        assert!(matches!(event, Some(AuraEvent::Connected)));
    }
}
