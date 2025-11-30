//! # TUI View Dynamics
//!
//! Reactive views that subscribe to database changes and maintain
//! up-to-date state for TUI rendering.

use std::sync::Arc;

use tokio::sync::{broadcast, RwLock};

use super::queries::{
    Channel, ChannelsQuery, Guardian, GuardiansQuery, Invitation, InvitationsQuery, Message,
    MessagesQuery, RecoveryQuery, RecoveryState, RecoveryStatus,
};

/// Generic view state wrapper
#[derive(Debug, Clone)]
pub struct ViewState<T> {
    /// Current data
    data: T,
    /// Whether the view is loading
    loading: bool,
    /// Last error (if any)
    error: Option<String>,
    /// Last update timestamp
    last_updated: u64,
}

impl<T: Default> Default for ViewState<T> {
    fn default() -> Self {
        Self {
            data: T::default(),
            loading: false,
            error: None,
            last_updated: 0,
        }
    }
}

impl<T: Clone> ViewState<T> {
    /// Create a new view state with initial data
    pub fn new(data: T) -> Self {
        Self {
            data,
            loading: false,
            error: None,
            last_updated: now_millis(),
        }
    }

    /// Get the current data
    pub fn data(&self) -> &T {
        &self.data
    }

    /// Check if loading
    pub fn is_loading(&self) -> bool {
        self.loading
    }

    /// Get the last error
    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// Get the last update timestamp
    pub fn last_updated(&self) -> u64 {
        self.last_updated
    }

    /// Update the data
    pub fn set_data(&mut self, data: T) {
        self.data = data;
        self.loading = false;
        self.error = None;
        self.last_updated = now_millis();
    }

    /// Set loading state
    pub fn set_loading(&mut self, loading: bool) {
        self.loading = loading;
    }

    /// Set error state
    pub fn set_error(&mut self, error: impl Into<String>) {
        self.error = Some(error.into());
        self.loading = false;
    }

    /// Clear error
    pub fn clear_error(&mut self) {
        self.error = None;
    }
}

/// Get current time in milliseconds
fn now_millis() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

// =============================================================================
// Chat View
// =============================================================================

/// State for a single channel
#[derive(Debug, Clone, Default)]
pub struct ChannelState {
    /// Channel metadata
    pub channel: Channel,
    /// Messages in this channel
    pub messages: Vec<Message>,
    /// Whether more messages are available
    pub has_more: bool,
    /// Scroll position (message index)
    pub scroll_position: usize,
}

/// Chat view with channels and messages
pub struct ChatView {
    /// All available channels
    channels: Arc<RwLock<ViewState<Vec<Channel>>>>,
    /// Currently selected channel ID
    selected_channel: Arc<RwLock<Option<String>>>,
    /// Channel states by ID
    channel_states: Arc<RwLock<std::collections::HashMap<String, ChannelState>>>,
    /// Update broadcaster
    update_tx: broadcast::Sender<ChatViewUpdate>,
}

/// Updates emitted by the chat view
#[derive(Debug, Clone)]
pub enum ChatViewUpdate {
    /// Channels list updated
    ChannelsUpdated,
    /// A channel's messages were updated
    MessagesUpdated {
        /// Channel ID
        channel_id: String,
    },
    /// Selected channel changed
    ChannelSelected {
        /// Selected channel ID
        channel_id: Option<String>,
    },
    /// New message received
    NewMessage {
        /// Channel ID
        channel_id: String,

        /// New message
        message: Message,
    },
    /// Error occurred
    Error {
        /// Error message
        message: String,
    },
}

impl ChatView {
    /// Create a new chat view
    pub fn new() -> Self {
        let (update_tx, _) = broadcast::channel(256);
        Self {
            channels: Arc::new(RwLock::new(ViewState::default())),
            selected_channel: Arc::new(RwLock::new(None)),
            channel_states: Arc::new(RwLock::new(std::collections::HashMap::new())),
            update_tx,
        }
    }

    /// Subscribe to view updates
    pub fn subscribe(&self) -> broadcast::Receiver<ChatViewUpdate> {
        self.update_tx.subscribe()
    }

    /// Get channels state
    pub async fn channels(&self) -> ViewState<Vec<Channel>> {
        self.channels.read().await.clone()
    }

    /// Get currently selected channel
    pub async fn selected_channel(&self) -> Option<String> {
        self.selected_channel.read().await.clone()
    }

    /// Get state for a specific channel
    pub async fn channel_state(&self, channel_id: &str) -> Option<ChannelState> {
        self.channel_states.read().await.get(channel_id).cloned()
    }

    // =========================================================================
    // Synchronous cached accessors (for deterministic/testable screen updates)
    // =========================================================================

    /// Get cached channels (non-blocking)
    pub fn cached_channels(&self) -> Option<Vec<Channel>> {
        self.channels.try_read().ok().map(|s| s.data().clone())
    }

    /// Get cached selected channel (non-blocking)
    pub fn cached_selected_channel(&self) -> Option<String> {
        self.selected_channel
            .try_read()
            .ok()
            .and_then(|s| s.clone())
    }

    /// Get cached channel state (non-blocking)
    pub fn cached_channel_state(&self, channel_id: &str) -> Option<ChannelState> {
        self.channel_states
            .try_read()
            .ok()
            .and_then(|s| s.get(channel_id).cloned())
    }

    /// Get cached messages for selected channel (non-blocking)
    pub fn cached_messages(&self) -> Option<Vec<Message>> {
        let channel_id = self.selected_channel.try_read().ok()?.clone()?;
        self.channel_states
            .try_read()
            .ok()
            .and_then(|s| s.get(&channel_id).map(|cs| cs.messages.clone()))
    }

    /// Select a channel
    pub async fn select_channel(&self, channel_id: Option<String>) {
        *self.selected_channel.write().await = channel_id.clone();
        let _ = self
            .update_tx
            .send(ChatViewUpdate::ChannelSelected { channel_id });
    }

    /// Update channels list
    pub async fn update_channels(&self, channels: Vec<Channel>) {
        self.channels.write().await.set_data(channels);
        let _ = self.update_tx.send(ChatViewUpdate::ChannelsUpdated);
    }

    /// Update messages for a channel
    pub async fn update_messages(&self, channel_id: &str, messages: Vec<Message>) {
        let mut states = self.channel_states.write().await;
        let state = states.entry(channel_id.to_string()).or_default();
        state.messages = messages;
        let _ = self.update_tx.send(ChatViewUpdate::MessagesUpdated {
            channel_id: channel_id.to_string(),
        });
    }

    /// Add a new message to a channel
    pub async fn add_message(&self, channel_id: &str, message: Message) {
        let mut states = self.channel_states.write().await;
        let state = states.entry(channel_id.to_string()).or_default();
        state.messages.push(message.clone());
        let _ = self.update_tx.send(ChatViewUpdate::NewMessage {
            channel_id: channel_id.to_string(),
            message,
        });
    }

    /// Set loading state for channels
    pub async fn set_loading(&self, loading: bool) {
        self.channels.write().await.set_loading(loading);
    }

    /// Set error state
    pub async fn set_error(&self, error: impl Into<String>) {
        let error = error.into();
        self.channels.write().await.set_error(&error);
        let _ = self
            .update_tx
            .send(ChatViewUpdate::Error { message: error });
    }

    /// Get the query for the current channels
    pub fn channels_query(&self) -> ChannelsQuery {
        ChannelsQuery::new()
    }

    /// Get the query for messages in the selected channel
    pub async fn messages_query(&self) -> Option<MessagesQuery> {
        let channel_id = self.selected_channel.read().await.clone()?;
        Some(MessagesQuery::new(channel_id).limit(100))
    }
}

impl Default for ChatView {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// Guardians View
// =============================================================================

/// Guardians view with status tracking
pub struct GuardiansView {
    /// Guardian list state
    guardians: Arc<RwLock<ViewState<Vec<Guardian>>>>,
    /// Threshold configuration
    threshold: Arc<RwLock<Option<ThresholdConfig>>>,
    /// Update broadcaster
    update_tx: broadcast::Sender<GuardiansViewUpdate>,
}

/// Threshold configuration
#[derive(Debug, Clone)]
pub struct ThresholdConfig {
    /// Required number of guardians for recovery
    pub threshold: u32,
    /// Total number of guardians
    pub total: u32,
}

/// Updates emitted by the guardians view
#[derive(Debug, Clone)]
pub enum GuardiansViewUpdate {
    /// Guardians list updated
    GuardiansUpdated,
    /// A guardian's status changed
    GuardianStatusChanged {
        /// Guardian ID
        guardian_id: String,
    },
    /// Threshold configuration updated
    ThresholdUpdated,
    /// Error occurred
    Error {
        /// Error message
        message: String,
    },
}

impl GuardiansView {
    /// Create a new guardians view
    pub fn new() -> Self {
        let (update_tx, _) = broadcast::channel(64);
        Self {
            guardians: Arc::new(RwLock::new(ViewState::default())),
            threshold: Arc::new(RwLock::new(None)),
            update_tx,
        }
    }

    /// Subscribe to view updates
    pub fn subscribe(&self) -> broadcast::Receiver<GuardiansViewUpdate> {
        self.update_tx.subscribe()
    }

    /// Get guardians state
    pub async fn guardians(&self) -> ViewState<Vec<Guardian>> {
        self.guardians.read().await.clone()
    }

    /// Get threshold configuration
    pub async fn threshold(&self) -> Option<ThresholdConfig> {
        self.threshold.read().await.clone()
    }

    // =========================================================================
    // Synchronous cached accessors (for deterministic/testable screen updates)
    // =========================================================================

    /// Get cached guardians (non-blocking)
    pub fn cached_guardians(&self) -> Option<Vec<Guardian>> {
        self.guardians.try_read().ok().map(|s| s.data().clone())
    }

    /// Get cached threshold (non-blocking)
    pub fn cached_threshold(&self) -> Option<ThresholdConfig> {
        self.threshold.try_read().ok().and_then(|s| s.clone())
    }

    /// Update guardians list
    pub async fn update_guardians(&self, guardians: Vec<Guardian>) {
        self.guardians.write().await.set_data(guardians);
        let _ = self.update_tx.send(GuardiansViewUpdate::GuardiansUpdated);
    }

    /// Update threshold configuration
    pub async fn update_threshold(&self, threshold: u32, total: u32) {
        *self.threshold.write().await = Some(ThresholdConfig { threshold, total });
        let _ = self.update_tx.send(GuardiansViewUpdate::ThresholdUpdated);
    }

    /// Set loading state
    pub async fn set_loading(&self, loading: bool) {
        self.guardians.write().await.set_loading(loading);
    }

    /// Set error state
    pub async fn set_error(&self, error: impl Into<String>) {
        let error = error.into();
        self.guardians.write().await.set_error(&error);
        let _ = self
            .update_tx
            .send(GuardiansViewUpdate::Error { message: error });
    }

    /// Get the query for guardians
    pub fn guardians_query(&self) -> GuardiansQuery {
        GuardiansQuery::new()
    }

    /// Get count of active guardians
    pub async fn active_count(&self) -> usize {
        self.guardians
            .read()
            .await
            .data()
            .iter()
            .filter(|g| g.status == super::queries::GuardianStatus::Active)
            .count()
    }
}

impl Default for GuardiansView {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// Recovery View
// =============================================================================

/// Recovery view with session tracking
pub struct RecoveryView {
    /// Recovery status state
    status: Arc<RwLock<ViewState<RecoveryStatus>>>,
    /// Update broadcaster
    update_tx: broadcast::Sender<RecoveryViewUpdate>,
}

/// Updates emitted by the recovery view
#[derive(Debug, Clone)]
pub enum RecoveryViewUpdate {
    /// Recovery status updated
    StatusUpdated,
    /// Recovery state changed
    StateChanged {
        /// New recovery state
        new_state: RecoveryState,
    },
    /// Guardian approved
    GuardianApproved {
        /// Guardian ID
        guardian_id: String,
    },
    /// Error occurred
    Error {
        /// Error message
        message: String,
    },
}

impl RecoveryView {
    /// Create a new recovery view
    pub fn new() -> Self {
        let (update_tx, _) = broadcast::channel(64);
        Self {
            status: Arc::new(RwLock::new(ViewState::default())),
            update_tx,
        }
    }

    /// Subscribe to view updates
    pub fn subscribe(&self) -> broadcast::Receiver<RecoveryViewUpdate> {
        self.update_tx.subscribe()
    }

    /// Get recovery status
    pub async fn status(&self) -> ViewState<RecoveryStatus> {
        self.status.read().await.clone()
    }

    // =========================================================================
    // Synchronous cached accessors (for deterministic/testable screen updates)
    // =========================================================================

    /// Get cached recovery status (non-blocking)
    pub fn cached_status(&self) -> Option<RecoveryStatus> {
        self.status.try_read().ok().map(|s| s.data().clone())
    }

    /// Update recovery status
    pub async fn update_status(&self, status: RecoveryStatus) {
        let old_state = self.status.read().await.data().state;
        let new_state = status.state;

        self.status.write().await.set_data(status);
        let _ = self.update_tx.send(RecoveryViewUpdate::StatusUpdated);

        if old_state != new_state {
            let _ = self
                .update_tx
                .send(RecoveryViewUpdate::StateChanged { new_state });
        }
    }

    /// Record a guardian approval
    pub async fn record_approval(&self, guardian_id: String) {
        let _ = self
            .update_tx
            .send(RecoveryViewUpdate::GuardianApproved { guardian_id });
    }

    /// Set loading state
    pub async fn set_loading(&self, loading: bool) {
        self.status.write().await.set_loading(loading);
    }

    /// Set error state
    pub async fn set_error(&self, error: impl Into<String>) {
        let error = error.into();
        self.status.write().await.set_error(&error);
        let _ = self
            .update_tx
            .send(RecoveryViewUpdate::Error { message: error });
    }

    /// Get the query for recovery status
    pub fn recovery_query(&self) -> RecoveryQuery {
        RecoveryQuery::active()
    }

    /// Check if recovery is in progress
    pub async fn is_in_progress(&self) -> bool {
        let state = self.status.read().await.data().state;
        matches!(
            state,
            RecoveryState::Initiated | RecoveryState::ThresholdMet | RecoveryState::InProgress
        )
    }

    /// Get progress as percentage (0-100)
    pub async fn progress_percent(&self) -> u32 {
        let status = self.status.read().await;
        let data = status.data();
        if data.threshold == 0 {
            return 0;
        }
        ((data.approvals_received as f64 / data.threshold as f64) * 100.0).min(100.0) as u32
    }
}

impl Default for RecoveryView {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// Invitations View
// =============================================================================

/// Invitations view
pub struct InvitationsView {
    /// Invitations list state
    invitations: Arc<RwLock<ViewState<Vec<Invitation>>>>,
    /// Update broadcaster
    update_tx: broadcast::Sender<InvitationsViewUpdate>,
}

/// Updates emitted by the invitations view
#[derive(Debug, Clone)]
pub enum InvitationsViewUpdate {
    /// Invitations list updated
    InvitationsUpdated,
    /// New invitation received
    NewInvitation {
        /// New invitation
        invitation: Invitation,
    },
    /// Invitation status changed
    InvitationStatusChanged {
        /// Invitation ID
        invitation_id: String,
    },
    /// Error occurred
    Error {
        /// Error message
        message: String,
    },
}

impl InvitationsView {
    /// Create a new invitations view
    pub fn new() -> Self {
        let (update_tx, _) = broadcast::channel(64);
        Self {
            invitations: Arc::new(RwLock::new(ViewState::default())),
            update_tx,
        }
    }

    /// Subscribe to view updates
    pub fn subscribe(&self) -> broadcast::Receiver<InvitationsViewUpdate> {
        self.update_tx.subscribe()
    }

    /// Get invitations state
    pub async fn invitations(&self) -> ViewState<Vec<Invitation>> {
        self.invitations.read().await.clone()
    }

    // =========================================================================
    // Synchronous cached accessors (for deterministic/testable screen updates)
    // =========================================================================

    /// Get cached invitations (non-blocking)
    pub fn cached_invitations(&self) -> Option<Vec<Invitation>> {
        self.invitations.try_read().ok().map(|s| s.data().clone())
    }

    /// Update invitations list
    pub async fn update_invitations(&self, invitations: Vec<Invitation>) {
        self.invitations.write().await.set_data(invitations);
        let _ = self
            .update_tx
            .send(InvitationsViewUpdate::InvitationsUpdated);
    }

    /// Add a new invitation
    pub async fn add_invitation(&self, invitation: Invitation) {
        {
            let mut state = self.invitations.write().await;
            let mut data = state.data().clone();
            data.push(invitation.clone());
            state.set_data(data);
        }
        let _ = self
            .update_tx
            .send(InvitationsViewUpdate::NewInvitation { invitation });
    }

    /// Set loading state
    pub async fn set_loading(&self, loading: bool) {
        self.invitations.write().await.set_loading(loading);
    }

    /// Set error state
    pub async fn set_error(&self, error: impl Into<String>) {
        let error = error.into();
        self.invitations.write().await.set_error(&error);
        let _ = self
            .update_tx
            .send(InvitationsViewUpdate::Error { message: error });
    }

    /// Get the query for invitations
    pub fn invitations_query(&self) -> InvitationsQuery {
        InvitationsQuery::new()
    }

    /// Get count of pending invitations
    pub async fn pending_count(&self) -> usize {
        self.invitations
            .read()
            .await
            .data()
            .iter()
            .filter(|i| i.status == super::queries::InvitationStatus::Pending)
            .count()
    }

    /// Get inbound pending invitations
    pub async fn pending_inbound(&self) -> Vec<Invitation> {
        self.invitations
            .read()
            .await
            .data()
            .iter()
            .filter(|i| {
                i.status == super::queries::InvitationStatus::Pending
                    && i.direction == super::queries::InvitationDirection::Inbound
            })
            .cloned()
            .collect()
    }
}

impl Default for InvitationsView {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_view_state_default() {
        let state: ViewState<Vec<String>> = ViewState::default();
        assert!(!state.is_loading());
        assert!(state.error().is_none());
        assert!(state.data().is_empty());
    }

    #[test]
    fn test_view_state_set_data() {
        let mut state = ViewState::default();
        state.set_data(vec!["hello".to_string()]);
        assert_eq!(state.data().len(), 1);
        assert!(!state.is_loading());
    }

    #[test]
    fn test_view_state_loading() {
        let mut state: ViewState<Vec<String>> = ViewState::default();
        state.set_loading(true);
        assert!(state.is_loading());
        state.set_loading(false);
        assert!(!state.is_loading());
    }

    #[test]
    fn test_view_state_error() {
        let mut state: ViewState<Vec<String>> = ViewState::default();
        state.set_error("Something went wrong");
        assert_eq!(state.error(), Some("Something went wrong"));
        state.clear_error();
        assert!(state.error().is_none());
    }

    #[tokio::test]
    async fn test_chat_view_creation() {
        let view = ChatView::new();
        assert!(view.selected_channel().await.is_none());
        let channels = view.channels().await;
        assert!(channels.data().is_empty());
    }

    #[tokio::test]
    async fn test_chat_view_select_channel() {
        let view = ChatView::new();
        view.select_channel(Some("general".to_string())).await;
        assert_eq!(view.selected_channel().await, Some("general".to_string()));
    }

    #[tokio::test]
    async fn test_chat_view_update_channels() {
        let view = ChatView::new();
        let channels = vec![Channel {
            id: "general".to_string(),
            name: "General".to_string(),
            ..Default::default()
        }];
        view.update_channels(channels).await;
        assert_eq!(view.channels().await.data().len(), 1);
    }

    #[tokio::test]
    async fn test_guardians_view_creation() {
        let view = GuardiansView::new();
        assert!(view.threshold().await.is_none());
        let guardians = view.guardians().await;
        assert!(guardians.data().is_empty());
    }

    #[tokio::test]
    async fn test_guardians_view_update_threshold() {
        let view = GuardiansView::new();
        view.update_threshold(2, 3).await;
        let threshold = view.threshold().await.unwrap();
        assert_eq!(threshold.threshold, 2);
        assert_eq!(threshold.total, 3);
    }

    #[tokio::test]
    async fn test_recovery_view_creation() {
        let view = RecoveryView::new();
        let status = view.status().await;
        assert_eq!(status.data().state, RecoveryState::None);
    }

    #[tokio::test]
    async fn test_recovery_view_progress() {
        let view = RecoveryView::new();
        view.update_status(RecoveryStatus {
            threshold: 2,
            approvals_received: 1,
            ..Default::default()
        })
        .await;
        assert_eq!(view.progress_percent().await, 50);
    }

    #[tokio::test]
    async fn test_invitations_view_creation() {
        let view = InvitationsView::new();
        let invitations = view.invitations().await;
        assert!(invitations.data().is_empty());
    }

    #[tokio::test]
    async fn test_invitations_view_pending_count() {
        let view = InvitationsView::new();
        view.update_invitations(vec![
            Invitation {
                id: "1".to_string(),
                status: super::super::queries::InvitationStatus::Pending,
                ..Default::default()
            },
            Invitation {
                id: "2".to_string(),
                status: super::super::queries::InvitationStatus::Accepted,
                ..Default::default()
            },
        ])
        .await;
        assert_eq!(view.pending_count().await, 1);
    }
}
