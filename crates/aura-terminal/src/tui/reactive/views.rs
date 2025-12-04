//! # TUI View Dynamics
//!
//! Reactive views that subscribe to database changes and maintain
//! up-to-date state for TUI rendering.

use super::queries::{
    Channel, ChannelsQuery, Guardian, GuardiansQuery, Invitation, InvitationsQuery, Message,
    MessagesQuery, RecoveryQuery, RecoveryState, RecoveryStatus,
};

// Import delta types from aura-agent reactive infrastructure
use aura_agent::reactive::{BlockDelta, ChatDelta, GuardianDelta, InvitationDelta, RecoveryDelta};

// Import reactive primitives for futures-signals integration
use super::signals::{ReactiveState, ReactiveVec};
use futures_signals::signal::{Mutable, Signal};
use futures_signals::signal_vec::SignalVec;

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
    use aura_effects::time::PhysicalTimeHandler;

    PhysicalTimeHandler::new().physical_time_now_ms()
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

/// Chat view with channels and messages (signal-based)
#[derive(Clone)]
pub struct ChatView {
    /// All available channels (reactive)
    channels: ReactiveVec<Channel>,
    /// Currently selected channel ID (reactive)
    selected_channel: ReactiveState<Option<String>>,
    /// Channel states by ID (reactive)
    channel_states: Mutable<std::collections::HashMap<String, ChannelState>>,
}

impl ChatView {
    /// Create a new chat view
    pub fn new() -> Self {
        Self {
            channels: ReactiveVec::new(),
            selected_channel: ReactiveState::new(None),
            channel_states: Mutable::new(std::collections::HashMap::new()),
        }
    }

    // =========================================================================
    // Signal Exposure Methods (for TUI rendering)
    // =========================================================================

    /// Get a signal that tracks all channels
    pub fn channels_signal(&self) -> impl SignalVec<Item = Channel> + Send + Sync + 'static
    where
        Channel: Send + Sync + 'static,
    {
        self.channels.signal_vec()
    }

    /// Get a signal that tracks the count of channels
    pub fn channels_count_signal(&self) -> impl Signal<Item = usize> + Send + Sync + 'static {
        self.channels.count_signal()
    }

    /// Get a signal that tracks the selected channel ID
    pub fn selected_channel_signal(
        &self,
    ) -> impl Signal<Item = Option<String>> + Send + Sync + 'static {
        self.selected_channel.signal()
    }

    /// Get a signal that tracks channel states
    pub fn channel_states_signal(
        &self,
    ) -> impl Signal<Item = std::collections::HashMap<String, ChannelState>> + Send + Sync + 'static
    where
        ChannelState: Clone + Send + Sync + 'static,
    {
        self.channel_states.signal_cloned()
    }

    // =========================================================================
    // Synchronous Accessors (for imperative code)
    // =========================================================================

    /// Get a snapshot of current channels
    pub fn get_channels(&self) -> Vec<Channel> {
        self.channels.get_cloned()
    }

    /// Get currently selected channel ID (snapshot)
    pub fn get_selected_channel(&self) -> Option<String> {
        self.selected_channel.get()
    }

    /// Get state for a specific channel (snapshot)
    pub fn get_channel_state(&self, channel_id: &str) -> Option<ChannelState> {
        self.channel_states.lock_ref().get(channel_id).cloned()
    }

    /// Select a channel (updates the signal automatically)
    pub fn select_channel(&self, channel_id: Option<String>) {
        self.selected_channel.set(channel_id);
    }

    /// Apply a delta to the chat view
    ///
    /// This method receives deltas from the ReactiveScheduler and updates
    /// the view's internal state accordingly. Deltas are generated by the
    /// ChatReduction function from journal facts.
    ///
    /// Signals automatically notify subscribers when state changes.
    ///
    /// See `aura-agent/src/reactive/scheduler.rs` for delta definitions.
    pub fn apply_delta(&self, delta: ChatDelta) {
        tracing::trace!("ChatView::apply_delta: {:?}", delta);

        match delta {
            ChatDelta::ChannelAdded {
                channel_id,
                name,
                topic,
                is_dm,
                member_count,
                created_at,
                creator_id: _,
            } => {
                tracing::debug!("Channel added: {} ({})", channel_id, name);

                // Push to reactive vec - automatically notifies subscribers
                self.channels.push(Channel {
                    id: channel_id,
                    name,
                    topic,
                    channel_type: aura_app::ChannelType::default(),
                    is_dm,
                    member_count,
                    last_message: None,
                    last_message_time: None,
                    last_activity: created_at,
                    unread_count: 0,
                });
            }
            ChatDelta::ChannelRemoved { channel_id } => {
                tracing::debug!("Channel removed: {}", channel_id);

                // Find and remove from reactive vec
                let channels = self.channels.get_cloned();
                if let Some(index) = channels.iter().position(|c| c.id == channel_id) {
                    self.channels.remove(index);
                }

                // Also remove channel state
                self.channel_states.lock_mut().remove(&channel_id);
            }
            ChatDelta::ChannelUpdated {
                channel_id,
                name,
                topic,
                member_count,
            } => {
                tracing::debug!("Channel updated: {}", channel_id);

                // Find and update channel in reactive vec
                let channels = self.channels.get_cloned();
                if let Some(index) = channels.iter().position(|c| c.id == channel_id) {
                    self.channels.update_at(index, |channel| {
                        if let Some(n) = name {
                            channel.name = n;
                        }
                        channel.topic = topic;
                        if let Some(mc) = member_count {
                            channel.member_count = mc;
                        }
                    });
                }
            }
            ChatDelta::MessageAdded {
                channel_id,
                message_id,
                sender_id,
                sender_name,
                content,
                timestamp,
                reply_to,
            } => {
                tracing::debug!("Message added to {}: {}", channel_id, message_id);

                // Update channel states - signals automatically notify
                let mut states = self.channel_states.lock_mut();
                let state = states.entry(channel_id).or_default();
                state.messages.push(Message {
                    id: message_id,
                    sender_id,
                    sender_name,
                    content,
                    timestamp,
                    reply_to,
                    ..Default::default()
                });
                // Drop lock to trigger signal notification
                drop(states);
            }
            ChatDelta::MessageRemoved {
                channel_id,
                message_id,
            } => {
                tracing::debug!("Message removed from {}: {}", channel_id, message_id);

                // Remove message from channel state
                let mut states = self.channel_states.lock_mut();
                if let Some(state) = states.get_mut(&channel_id) {
                    state.messages.retain(|m| m.id != message_id);
                }
                // Drop lock to trigger signal notification
                drop(states);
            }
        }
    }

    // =========================================================================
    // Helper Methods
    // =========================================================================

    /// Get the query for the current channels
    pub fn channels_query(&self) -> ChannelsQuery {
        ChannelsQuery::new()
    }

    /// Get the query for messages in the selected channel
    pub fn messages_query(&self) -> Option<MessagesQuery> {
        let channel_id = self.selected_channel.get()?;
        Some(MessagesQuery::new(channel_id).limit(100))
    }

    /// Update the channels list (replaces all channels)
    pub fn update_channels(&self, channels: Vec<Channel>) {
        self.channels.replace(channels);
    }

    /// Update messages for a channel
    pub fn update_messages(&self, channel_id: &str, messages: Vec<Message>) {
        let mut states = self.channel_states.lock_mut();
        let state = states.entry(channel_id.to_string()).or_default();
        state.messages = messages;
    }

    /// Add a message to a channel (for manual message insertion)
    pub fn add_message(&self, channel_id: &str, message: Message) {
        let mut states = self.channel_states.lock_mut();
        let state = states.entry(channel_id.to_string()).or_default();
        state.messages.push(message);
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

/// Guardians view with status tracking (signal-based)
#[derive(Clone)]
pub struct GuardiansView {
    /// Guardian list state (reactive)
    guardians: ReactiveVec<Guardian>,
    /// Threshold configuration (reactive)
    threshold: ReactiveState<Option<ThresholdConfig>>,
}

/// Threshold configuration
#[derive(Debug, Clone)]
pub struct ThresholdConfig {
    /// Required number of guardians for recovery
    pub threshold: u32,
    /// Total number of guardians
    pub total: u32,
}

impl GuardiansView {
    /// Create a new guardians view
    pub fn new() -> Self {
        Self {
            guardians: ReactiveVec::new(),
            threshold: ReactiveState::new(None),
        }
    }

    // =========================================================================
    // Signal Exposure Methods (for TUI rendering)
    // =========================================================================

    /// Get a signal that tracks all guardians
    pub fn guardians_signal(&self) -> impl SignalVec<Item = Guardian> + Send + Sync + 'static
    where
        Guardian: Send + Sync + 'static,
    {
        self.guardians.signal_vec()
    }

    /// Get a signal that tracks the guardian count
    pub fn guardians_count_signal(&self) -> impl Signal<Item = usize> + Send + Sync + 'static {
        self.guardians.count_signal()
    }

    /// Get a signal that tracks threshold configuration
    pub fn threshold_signal(
        &self,
    ) -> impl Signal<Item = Option<ThresholdConfig>> + Send + Sync + 'static
    where
        ThresholdConfig: Send + Sync + 'static,
    {
        self.threshold.signal()
    }

    // =========================================================================
    // Synchronous Accessors (for imperative code)
    // =========================================================================

    /// Get a snapshot of current guardians
    pub fn get_guardians(&self) -> Vec<Guardian> {
        self.guardians.get_cloned()
    }

    /// Get current threshold configuration (snapshot)
    pub fn get_threshold(&self) -> Option<ThresholdConfig> {
        self.threshold.get()
    }

    /// Apply a delta to the guardians view
    ///
    /// Receives deltas from ReactiveScheduler generated by GuardianReduction.
    /// Signals automatically notify subscribers when state changes.
    pub fn apply_delta(&self, delta: GuardianDelta) {
        tracing::trace!("GuardiansView::apply_delta: {:?}", delta);

        match delta {
            GuardianDelta::GuardianAdded {
                authority_id,
                name,
                added_at,
                share_index,
            } => {
                tracing::debug!("Guardian added: {} ({})", authority_id, name);

                // Push to reactive vec - automatically notifies subscribers
                self.guardians.push(Guardian {
                    authority_id,
                    name,
                    status: super::queries::GuardianStatus::Active,
                    added_at,
                    last_seen: Some(added_at),
                    share_index,
                });
            }
            GuardianDelta::GuardianStatusChanged {
                authority_id,
                old_status: _,
                new_status,
                last_seen,
            } => {
                tracing::debug!("Guardian {} status changed to {}", authority_id, new_status);

                // Find and update guardian in reactive vec
                let guardians = self.guardians.get_cloned();
                if let Some(index) = guardians
                    .iter()
                    .position(|g| g.authority_id == authority_id)
                {
                    self.guardians.update_at(index, |guardian| {
                        // Parse status string to enum
                        guardian.status = match new_status.as_str() {
                            "active" => super::queries::GuardianStatus::Active,
                            "offline" => super::queries::GuardianStatus::Offline,
                            "pending" => super::queries::GuardianStatus::Pending,
                            "declined" => super::queries::GuardianStatus::Declined,
                            "removed" => super::queries::GuardianStatus::Removed,
                            _ => super::queries::GuardianStatus::Active,
                        };
                        if let Some(ts) = last_seen {
                            guardian.last_seen = Some(ts);
                        }
                    });
                }
            }
            GuardianDelta::ThresholdUpdated { threshold, total } => {
                tracing::debug!("Threshold updated: {}/{}", threshold, total);
                self.threshold
                    .set(Some(ThresholdConfig { threshold, total }));
            }
            GuardianDelta::GuardianRemoved { authority_id } => {
                tracing::debug!("Guardian removed: {}", authority_id);

                // Find and remove from reactive vec
                let guardians = self.guardians.get_cloned();
                if let Some(index) = guardians
                    .iter()
                    .position(|g| g.authority_id == authority_id)
                {
                    self.guardians.remove(index);
                }
            }
        }
    }

    // =========================================================================
    // Helper Methods
    // =========================================================================

    /// Get the query for guardians
    pub fn guardians_query(&self) -> GuardiansQuery {
        GuardiansQuery::new()
    }

    /// Get count of active guardians (snapshot)
    pub fn active_count(&self) -> usize {
        self.guardians
            .get_cloned()
            .iter()
            .filter(|g| g.status == super::queries::GuardianStatus::Active)
            .count()
    }

    /// Update the guardians list (replaces all guardians)
    pub fn update_guardians(&self, guardians: Vec<Guardian>) {
        self.guardians.replace(guardians);
    }

    /// Update threshold configuration
    pub fn update_threshold(&self, threshold: u32, total: u32) {
        self.threshold
            .set(Some(ThresholdConfig { threshold, total }));
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

/// Recovery view with session tracking (signal-based)
#[derive(Clone)]
pub struct RecoveryView {
    /// Recovery status state (reactive)
    status: ReactiveState<RecoveryStatus>,
}

impl RecoveryView {
    /// Create a new recovery view
    pub fn new() -> Self {
        Self {
            status: ReactiveState::new(RecoveryStatus::default()),
        }
    }

    // =========================================================================
    // Signal Exposure Methods (for TUI rendering)
    // =========================================================================

    /// Get a signal that tracks the recovery status
    pub fn status_signal(&self) -> impl Signal<Item = RecoveryStatus> + Send + Sync + 'static
    where
        RecoveryStatus: Send + Sync + 'static,
    {
        self.status.signal()
    }

    // =========================================================================
    // Synchronous Accessors (for imperative code)
    // =========================================================================

    /// Get a snapshot of the current recovery status
    pub fn get_status(&self) -> RecoveryStatus {
        self.status.get()
    }

    /// Apply a delta to the recovery view
    ///
    /// Receives deltas from ReactiveScheduler generated by RecoveryReduction.
    pub fn apply_delta(&self, delta: RecoveryDelta) {
        tracing::trace!("RecoveryView::apply_delta: {:?}", delta);

        match delta {
            RecoveryDelta::SessionInitiated {
                session_id,
                threshold,
                total_guardians,
                started_at,
                expires_at,
            } => {
                tracing::debug!(
                    "Recovery session initiated: {} ({}/{})",
                    session_id,
                    threshold,
                    total_guardians
                );

                // Update reactive state - automatically notifies subscribers
                self.status.update(|status| {
                    status.state = RecoveryState::Initiated;
                    status.threshold = threshold;
                    status.total_guardians = total_guardians;
                    status.approvals_received = 0;
                    status.session_id = Some(session_id);
                    status.started_at = Some(started_at);
                    status.expires_at = expires_at;
                });
            }
            RecoveryDelta::ApprovalReceived {
                session_id: _,
                guardian_id,
                guardian_name: _,
                approved_at: _,
                approval_count,
            } => {
                tracing::debug!("Guardian approval received: {}", guardian_id);

                // Update reactive state - automatically notifies subscribers
                self.status.update(|status| {
                    status.approvals_received = approval_count;

                    // Update state based on threshold
                    if approval_count >= status.threshold {
                        status.state = RecoveryState::ThresholdMet;
                    } else {
                        status.state = RecoveryState::InProgress;
                    }
                });
            }
            RecoveryDelta::ThresholdMet {
                session_id: _,
                approval_count,
                threshold: _,
            } => {
                tracing::debug!("Recovery threshold met with {} approvals", approval_count);

                // Update reactive state - automatically notifies subscribers
                self.status.update(|status| {
                    status.state = RecoveryState::ThresholdMet;
                    status.approvals_received = approval_count;
                });
            }
            RecoveryDelta::SessionCompleted {
                session_id: _,
                completed_at: _,
            } => {
                tracing::debug!("Recovery completed");

                // Update reactive state - automatically notifies subscribers
                self.status.update(|status| {
                    status.state = RecoveryState::Completed;
                });
            }
            RecoveryDelta::SessionFailed {
                session_id: _,
                reason,
                failed_at: _,
            } => {
                tracing::debug!("Recovery failed: {}", reason);

                // Update reactive state - automatically notifies subscribers
                self.status.update(|status| {
                    status.state = RecoveryState::Failed;
                });
            }
            RecoveryDelta::SessionCancelled {
                session_id: _,
                cancelled_at: _,
            } => {
                tracing::debug!("Recovery cancelled");

                // Update reactive state - automatically notifies subscribers
                self.status.update(|status| {
                    status.state = RecoveryState::Cancelled;
                });
            }
        }
    }

    /// Get the query for recovery status
    pub fn recovery_query(&self) -> RecoveryQuery {
        RecoveryQuery::active()
    }

    /// Check if recovery is in progress
    pub fn is_in_progress(&self) -> bool {
        let status = self.status.get();
        matches!(
            status.state,
            RecoveryState::Initiated | RecoveryState::ThresholdMet | RecoveryState::InProgress
        )
    }

    /// Get progress as percentage (0-100)
    pub fn progress_percent(&self) -> u32 {
        let status = self.status.get();
        if status.threshold == 0 {
            return 0;
        }
        ((status.approvals_received as f64 / status.threshold as f64) * 100.0).min(100.0) as u32
    }

    /// Update recovery status
    pub fn update_status(&self, status: RecoveryStatus) {
        self.status.set(status);
    }

    /// Get cached status synchronously (for legacy compatibility)
    pub fn cached_status(&self) -> Option<RecoveryStatus> {
        Some(self.status.get())
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

/// Invitations view (signal-based)
#[derive(Clone)]
pub struct InvitationsView {
    /// Invitations list state (reactive)
    invitations: ReactiveVec<Invitation>,
}

impl InvitationsView {
    /// Create a new invitations view
    pub fn new() -> Self {
        Self {
            invitations: ReactiveVec::new(),
        }
    }

    // =========================================================================
    // Signal Exposure Methods (for TUI rendering)
    // =========================================================================

    /// Get a signal that tracks all invitations
    pub fn invitations_signal(&self) -> impl SignalVec<Item = Invitation> + Send + Sync + 'static
    where
        Invitation: Send + Sync + 'static,
    {
        self.invitations.signal_vec()
    }

    /// Get a signal that tracks the count of invitations
    pub fn invitations_count_signal(&self) -> impl Signal<Item = usize> + Send + Sync + 'static {
        self.invitations.count_signal()
    }

    // =========================================================================
    // Synchronous Accessors (for imperative code)
    // =========================================================================

    /// Get a snapshot of all invitations
    pub fn get_invitations(&self) -> Vec<Invitation> {
        self.invitations.get_cloned()
    }

    /// Apply a delta to the invitations view
    ///
    /// Receives deltas from ReactiveScheduler generated by InvitationReduction.
    pub fn apply_delta(&self, delta: InvitationDelta) {
        tracing::trace!("InvitationsView::apply_delta: {:?}", delta);

        match delta {
            InvitationDelta::InvitationAdded {
                invitation_id,
                direction,
                other_party_id,
                other_party_name,
                invitation_type,
                created_at,
                expires_at,
                message,
            } => {
                tracing::debug!("Invitation added: {} ({})", invitation_id, invitation_type);

                // Incremental update: Add invitation
                let invitation = Invitation {
                    id: invitation_id.clone(),
                    direction: if direction == "inbound" {
                        super::queries::InvitationDirection::Inbound
                    } else {
                        super::queries::InvitationDirection::Outbound
                    },
                    other_party_id,
                    other_party_name,
                    invitation_type: match invitation_type.as_str() {
                        "guardian" => super::queries::InvitationType::Guardian,
                        "channel" => super::queries::InvitationType::Channel,
                        "contact" => super::queries::InvitationType::Contact,
                        _ => super::queries::InvitationType::Guardian,
                    },
                    status: super::queries::InvitationStatus::Pending,
                    created_at,
                    expires_at,
                    message,
                };

                // ReactiveVec automatically notifies subscribers
                self.invitations.push(invitation);
            }
            InvitationDelta::InvitationStatusChanged {
                invitation_id,
                old_status: _,
                new_status,
                changed_at: _,
            } => {
                tracing::debug!(
                    "Invitation {} status changed to {}",
                    invitation_id,
                    new_status
                );

                // Find and update the invitation
                let invitations = self.invitations.get_cloned();
                if let Some(index) = invitations.iter().position(|i| i.id == invitation_id) {
                    self.invitations.update_at(index, |invitation| {
                        invitation.status = match new_status.as_str() {
                            "accepted" => super::queries::InvitationStatus::Accepted,
                            "declined" => super::queries::InvitationStatus::Declined,
                            "expired" => super::queries::InvitationStatus::Expired,
                            "cancelled" => super::queries::InvitationStatus::Cancelled,
                            _ => super::queries::InvitationStatus::Pending,
                        };
                    });
                }
            }
            InvitationDelta::InvitationRemoved { invitation_id } => {
                tracing::debug!("Invitation removed: {}", invitation_id);

                // Find and remove the invitation
                let invitations = self.invitations.get_cloned();
                if let Some(index) = invitations.iter().position(|i| i.id == invitation_id) {
                    self.invitations.remove(index);
                }
            }
        }
    }

    /// Add a new invitation
    pub fn add_invitation(&self, invitation: Invitation) {
        self.invitations.push(invitation);
    }

    /// Get the query for invitations
    pub fn invitations_query(&self) -> InvitationsQuery {
        InvitationsQuery::new()
    }

    /// Get count of pending invitations
    pub fn pending_count(&self) -> usize {
        self.invitations
            .get_cloned()
            .iter()
            .filter(|i| i.status == super::queries::InvitationStatus::Pending)
            .count()
    }

    /// Get inbound pending invitations
    pub fn pending_inbound(&self) -> Vec<Invitation> {
        self.invitations
            .get_cloned()
            .iter()
            .filter(|i| {
                i.status == super::queries::InvitationStatus::Pending
                    && i.direction == super::queries::InvitationDirection::Inbound
            })
            .cloned()
            .collect()
    }

    /// Update the invitations list
    pub fn update_invitations(&self, invitations: Vec<Invitation>) {
        self.invitations.replace(invitations);
    }
}

impl Default for InvitationsView {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// Welcome View
// =============================================================================

/// Welcome view with account status using reactive signals
#[derive(Clone)]
pub struct WelcomeView {
    /// Whether an account exists
    has_account: ReactiveState<bool>,
    /// Account authority ID (if exists)
    authority_id: ReactiveState<Option<String>>,
}

impl WelcomeView {
    /// Create a new welcome view
    pub fn new() -> Self {
        Self {
            has_account: ReactiveState::new(false),
            authority_id: ReactiveState::new(None),
        }
    }

    /// Get account status
    pub fn has_account(&self) -> bool {
        self.has_account.get()
    }

    /// Get authority ID
    pub fn authority_id(&self) -> Option<String> {
        self.authority_id.get()
    }

    /// Set account status (for initialization)
    pub fn set_account_status(&self, has_account: bool, authority_id: Option<String>) {
        self.has_account.set(has_account);
        self.authority_id.set(authority_id);
    }

    /// Signal for account status changes
    pub fn has_account_signal(&self) -> impl Signal<Item = bool> + Send + Sync + 'static {
        self.has_account.signal()
    }

    /// Signal for authority ID changes
    pub fn authority_id_signal(
        &self,
    ) -> impl Signal<Item = Option<String>> + Send + Sync + 'static {
        self.authority_id.signal()
    }
}

impl Default for WelcomeView {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// Block View
// =============================================================================

/// Block information for display
#[derive(Debug, Clone, Default)]
pub struct BlockInfo {
    /// Block identifier
    pub id: String,
    /// Block name
    pub name: Option<String>,
    /// Description or topic
    pub description: Option<String>,
    /// When the block was created
    pub created_at: u64,
}

/// A resident of a block
#[derive(Debug, Clone, Default)]
pub struct Resident {
    /// Authority ID
    pub authority_id: String,
    /// Display name
    pub name: String,
    /// Whether this is the current user
    pub is_self: bool,
    /// Whether this resident is online
    pub is_online: bool,
    /// Role in the block (resident, steward)
    pub role: ResidentRole,
}

/// Resident role in a block
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ResidentRole {
    /// Regular resident with no elevated privileges
    #[default]
    Resident,
    /// Steward with elevated maintenance privileges
    Steward,
}

/// Storage usage info
#[derive(Debug, Clone, Default)]
pub struct StorageInfo {
    /// Used storage in bytes
    pub used_bytes: u64,
    /// Total available storage in bytes
    pub total_bytes: u64,
}

/// Block view with storage and membership tracking (signal-based)
#[derive(Clone)]
pub struct BlockView {
    /// Block information (reactive)
    block: ReactiveState<Option<BlockInfo>>,
    /// Block residents (reactive)
    residents: ReactiveVec<Resident>,
    /// Block channels from chat (reactive)
    channels: ReactiveVec<Channel>,
    /// Storage info (reactive)
    storage: ReactiveState<StorageInfo>,
    /// Number of neighborhoods this block belongs to (reactive)
    neighborhood_count: ReactiveState<u32>,
    /// Whether current user is a resident (reactive)
    is_resident: ReactiveState<bool>,
    /// Whether current user is a steward (reactive)
    is_steward: ReactiveState<bool>,
}

impl BlockView {
    /// Create a new block view
    pub fn new() -> Self {
        Self {
            block: ReactiveState::new(None),
            residents: ReactiveVec::new(),
            channels: ReactiveVec::new(),
            storage: ReactiveState::new(StorageInfo::default()),
            neighborhood_count: ReactiveState::new(0),
            is_resident: ReactiveState::new(false),
            is_steward: ReactiveState::new(false),
        }
    }

    // Signal Exposure Methods
    /// Reactive signal for the current block metadata.
    pub fn block_signal(&self) -> impl Signal<Item = Option<BlockInfo>> + Send + Sync + 'static {
        self.block.signal()
    }

    /// Reactive stream of all residents within the block.
    pub fn residents_signal(&self) -> impl SignalVec<Item = Resident> + Send + Sync + 'static {
        self.residents.signal_vec()
    }

    /// Reactive count of residents.
    pub fn residents_count_signal(&self) -> impl Signal<Item = usize> + Send + Sync + 'static {
        self.residents.count_signal()
    }

    /// Reactive stream of channels belonging to this block.
    pub fn channels_signal(&self) -> impl SignalVec<Item = Channel> + Send + Sync + 'static {
        self.channels.signal_vec()
    }

    /// Reactive signal of storage statistics for the block.
    pub fn storage_signal(&self) -> impl Signal<Item = StorageInfo> + Send + Sync + 'static {
        self.storage.signal()
    }

    /// Reactive count of neighborhoods containing this block.
    pub fn neighborhood_count_signal(&self) -> impl Signal<Item = u32> + Send + Sync + 'static {
        self.neighborhood_count.signal()
    }

    /// Reactive flag indicating whether the current user is a resident.
    pub fn is_resident_signal(&self) -> impl Signal<Item = bool> + Send + Sync + 'static {
        self.is_resident.signal()
    }

    /// Reactive flag indicating whether the current user is a steward.
    pub fn is_steward_signal(&self) -> impl Signal<Item = bool> + Send + Sync + 'static {
        self.is_steward.signal()
    }

    // Synchronous Accessors
    /// Snapshot of the current block metadata, if any.
    pub fn get_block(&self) -> Option<BlockInfo> {
        self.block.get()
    }

    /// Snapshot list of residents.
    pub fn get_residents(&self) -> Vec<Resident> {
        self.residents.get_cloned()
    }

    /// Snapshot list of channels.
    pub fn get_channels(&self) -> Vec<Channel> {
        self.channels.get_cloned()
    }

    /// Snapshot of storage statistics.
    pub fn get_storage(&self) -> StorageInfo {
        self.storage.get()
    }

    /// Snapshot count of neighborhoods containing this block.
    pub fn get_neighborhood_count(&self) -> u32 {
        self.neighborhood_count.get()
    }

    /// Snapshot flag indicating whether the current user is a resident.
    pub fn get_is_resident(&self) -> bool {
        self.is_resident.get()
    }

    /// Snapshot flag indicating whether the current user is a steward.
    pub fn get_is_steward(&self) -> bool {
        self.is_steward.get()
    }

    /// Apply a delta to the block view
    ///
    /// Receives deltas from ReactiveScheduler generated by BlockReduction.
    pub fn apply_delta(&self, delta: BlockDelta) {
        tracing::trace!("BlockView::apply_delta: {:?}", delta);

        match delta {
            BlockDelta::BlockCreated {
                block_id,
                name,
                created_at,
                creator_id: _,
            } => {
                tracing::debug!("Block created: {} ({})", block_id, name);

                // ReactiveState automatically notifies subscribers
                self.block.set(Some(BlockInfo {
                    id: block_id,
                    name: Some(name),
                    description: None,
                    created_at,
                }));
            }
            BlockDelta::ResidentAdded {
                authority_id,
                name,
                joined_at: _,
            } => {
                tracing::debug!("Resident added: {} ({})", authority_id, name);

                // ReactiveVec automatically notifies subscribers
                self.residents.push(Resident {
                    authority_id,
                    name,
                    is_self: false, // Would need current authority context
                    is_online: false,
                    role: ResidentRole::Resident,
                });
            }
            BlockDelta::ResidentRemoved {
                authority_id,
                left_at: _,
            } => {
                tracing::debug!("Resident removed: {}", authority_id);

                // Find and remove the resident
                let residents = self.residents.get_cloned();
                if let Some(index) = residents
                    .iter()
                    .position(|r| r.authority_id == authority_id)
                {
                    self.residents.remove(index);
                }
            }
            BlockDelta::StorageUpdated {
                used_bytes,
                total_bytes,
                updated_at: _,
            } => {
                tracing::debug!("Storage updated: {}/{} bytes", used_bytes, total_bytes);

                // ReactiveState automatically notifies subscribers
                self.storage.set(StorageInfo {
                    used_bytes,
                    total_bytes,
                });
            }
        }
    }
}

impl Default for BlockView {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// Contacts View
// =============================================================================

/// A contact in the contacts list
#[derive(Debug, Clone, Default)]
pub struct Contact {
    /// Authority ID
    pub authority_id: String,
    /// Petname (local display name)
    pub petname: String,
    /// Their suggested display name
    pub suggested_name: Option<String>,
    /// Whether contact is online
    pub is_online: Option<bool>,
    /// When added
    pub added_at: u64,
    /// Last interaction time
    pub last_seen: Option<u64>,
    /// Whether there's a pending suggestion
    pub has_pending_suggestion: bool,
}

/// Suggestion policy for contacts
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SuggestionPolicy {
    /// Automatically accept contact suggestions
    #[default]
    AutoAccept,
    /// Prompt the user before accepting suggestions
    PromptFirst,
    /// Ignore incoming suggestions
    Ignore,
}

/// Contact suggestion (what the user shares about themselves)
#[derive(Debug, Clone, Default)]
pub struct MySuggestion {
    /// Display name
    pub display_name: Option<String>,
    /// Status message
    pub status: Option<String>,
}

/// Contacts view using reactive signals
#[derive(Clone)]
pub struct ContactsView {
    /// Contacts list
    contacts: ReactiveVec<Contact>,
    /// Suggestion policy
    policy: ReactiveState<SuggestionPolicy>,
    /// User's own suggestion
    my_suggestion: ReactiveState<MySuggestion>,
}

impl ContactsView {
    /// Create a new contacts view
    pub fn new() -> Self {
        Self {
            contacts: ReactiveVec::new(),
            policy: ReactiveState::new(SuggestionPolicy::default()),
            my_suggestion: ReactiveState::new(MySuggestion::default()),
        }
    }

    /// Get the full contacts list
    pub fn contacts(&self) -> Vec<Contact> {
        self.contacts.get_cloned()
    }

    /// Get the current suggestion policy
    pub fn policy(&self) -> SuggestionPolicy {
        self.policy.get()
    }

    /// Get the user's own suggestion payload
    pub fn my_suggestion(&self) -> MySuggestion {
        self.my_suggestion.get()
    }

    /// Set contacts list (for initialization/bulk updates)
    pub fn set_contacts(&self, contacts: Vec<Contact>) {
        self.contacts.replace(contacts);
    }

    /// Add a contact
    pub fn add_contact(&self, contact: Contact) {
        self.contacts.push(contact);
    }

    /// Remove a contact by authority ID
    pub fn remove_contact(&self, authority_id: &str) {
        let contacts = self.contacts.get_cloned();
        if let Some(index) = contacts.iter().position(|c| c.authority_id == authority_id) {
            self.contacts.remove(index);
        }
    }

    /// Update a contact
    pub fn update_contact<F>(&self, authority_id: &str, f: F)
    where
        F: FnOnce(&mut Contact),
    {
        let contacts = self.contacts.get_cloned();
        if let Some(index) = contacts.iter().position(|c| c.authority_id == authority_id) {
            self.contacts.update_at(index, f);
        }
    }

    /// Set suggestion policy
    pub fn set_policy(&self, policy: SuggestionPolicy) {
        self.policy.set(policy);
    }

    /// Set user's own suggestion
    pub fn set_my_suggestion(&self, suggestion: MySuggestion) {
        self.my_suggestion.set(suggestion);
    }

    /// Get count of contacts with pending suggestions
    pub fn pending_suggestion_count(&self) -> usize {
        self.contacts
            .get_cloned()
            .iter()
            .filter(|c| c.has_pending_suggestion)
            .count()
    }

    // Signal exposure for reactive UI

    /// Signal for contacts list changes
    pub fn contacts_signal(&self) -> impl SignalVec<Item = Contact> + Send + Sync + 'static {
        self.contacts.signal_vec()
    }

    /// Signal for policy changes
    pub fn policy_signal(&self) -> impl Signal<Item = SuggestionPolicy> + Send + Sync + 'static {
        self.policy.signal()
    }

    /// Signal for my_suggestion changes
    pub fn my_suggestion_signal(&self) -> impl Signal<Item = MySuggestion> + Send + Sync + 'static {
        self.my_suggestion.signal()
    }

    /// Signal for contact count
    pub fn contacts_count_signal(&self) -> impl Signal<Item = usize> + Send + Sync + 'static {
        self.contacts.count_signal()
    }
}

impl Default for ContactsView {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// Neighborhood View
// =============================================================================

/// Block summary for neighborhood display
#[derive(Debug, Clone, Default)]
pub struct NeighborhoodBlock {
    /// Block ID
    pub id: String,
    /// Block name
    pub name: Option<String>,
    /// Resident count
    pub resident_count: u8,
    /// Max residents (usually 8)
    pub max_residents: u8,
    /// Whether this is the user's home block
    pub is_home: bool,
    /// Whether user can enter this block
    pub can_enter: bool,
    /// Whether user is currently at this block
    pub is_current: bool,
}

/// Adjacency between blocks
#[derive(Debug, Clone, Default)]
pub struct BlockAdjacency {
    /// First block ID
    pub block_a: String,
    /// Second block ID
    pub block_b: String,
}

/// Traversal depth in a block
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TraversalDepth {
    /// Street-level access
    #[default]
    Street,
    /// Frontage (immediate neighboring blocks)
    Frontage,
    /// Interior (deep traversal)
    Interior,
}

/// Current position in neighborhood traversal
#[derive(Debug, Clone, Default)]
pub struct TraversalPosition {
    /// Current neighborhood ID
    pub neighborhood_id: Option<String>,
    /// Current block ID (None = on street)
    pub block_id: Option<String>,
    /// Depth of access
    pub depth: TraversalDepth,
    /// When this position was entered
    pub entered_at: u64,
}

/// Neighborhood view
#[derive(Clone)]
pub struct NeighborhoodView {
    /// Neighborhood ID
    neighborhood_id: ReactiveState<Option<String>>,
    /// Neighborhood name
    neighborhood_name: ReactiveState<Option<String>>,
    /// Blocks in neighborhood
    blocks: ReactiveVec<NeighborhoodBlock>,
    /// Adjacencies between blocks
    adjacencies: ReactiveVec<BlockAdjacency>,
    /// Current traversal position
    position: ReactiveState<TraversalPosition>,
}

/// Updates emitted by the neighborhood view
#[derive(Debug, Clone)]
pub enum NeighborhoodViewUpdate {
    /// Neighborhood info changed
    NeighborhoodChanged,
    /// Blocks updated
    BlocksUpdated,
    /// Adjacencies updated
    AdjacenciesUpdated,
    /// Position changed
    PositionChanged,
    /// Error occurred
    Error {
        /// Human-readable description of the failure
        message: String,
    },
}

impl NeighborhoodView {
    /// Create a new neighborhood view
    pub fn new() -> Self {
        Self {
            neighborhood_id: ReactiveState::new(None),
            neighborhood_name: ReactiveState::new(None),
            blocks: ReactiveVec::new(),
            adjacencies: ReactiveVec::new(),
            position: ReactiveState::new(TraversalPosition::default()),
        }
    }

    // Getters

    /// Get the current neighborhood identifier
    pub fn neighborhood_id(&self) -> Option<String> {
        self.neighborhood_id.get()
    }

    /// Get the current neighborhood display name
    pub fn neighborhood_name(&self) -> Option<String> {
        self.neighborhood_name.get()
    }

    /// Get all blocks in the neighborhood
    pub fn blocks(&self) -> Vec<NeighborhoodBlock> {
        self.blocks.get_cloned()
    }

    /// Get adjacency pairs between blocks
    pub fn adjacencies(&self) -> Vec<BlockAdjacency> {
        self.adjacencies.get_cloned()
    }

    /// Get the active traversal position
    pub fn position(&self) -> TraversalPosition {
        self.position.get()
    }

    // Mutations

    /// Set neighborhood information
    pub fn set_neighborhood_info(&self, id: Option<String>, name: Option<String>) {
        if let Some(id) = id {
            self.neighborhood_id.set(Some(id));
        }
        if let Some(name) = name {
            self.neighborhood_name.set(Some(name));
        }
    }

    /// Set blocks
    pub fn set_blocks(&self, blocks: Vec<NeighborhoodBlock>) {
        self.blocks.replace(blocks);
    }

    /// Add a block
    pub fn add_block(&self, block: NeighborhoodBlock) {
        self.blocks.push(block);
    }

    /// Remove a block by ID
    pub fn remove_block(&self, block_id: &str) {
        let blocks = self.blocks.get_cloned();
        if let Some(index) = blocks.iter().position(|b| b.id == block_id) {
            self.blocks.remove(index);
        }
    }

    /// Update a block by ID
    pub fn update_block<F>(&self, block_id: &str, f: F)
    where
        F: FnOnce(&mut NeighborhoodBlock),
    {
        let blocks = self.blocks.get_cloned();
        if let Some(index) = blocks.iter().position(|b| b.id == block_id) {
            self.blocks.update_at(index, f);
        }
    }

    /// Set adjacencies
    pub fn set_adjacencies(&self, adjacencies: Vec<BlockAdjacency>) {
        self.adjacencies.replace(adjacencies);
    }

    /// Add an adjacency
    pub fn add_adjacency(&self, adjacency: BlockAdjacency) {
        self.adjacencies.push(adjacency);
    }

    /// Set traversal position
    pub fn set_position(&self, position: TraversalPosition) {
        self.position.set(position);
    }

    // Derived state

    /// Get adjacent blocks for a given block
    pub fn get_adjacent_blocks(&self, block_id: &str) -> Vec<NeighborhoodBlock> {
        let adjacencies = self.adjacencies.get_cloned();
        let adjacent_ids: Vec<String> = adjacencies
            .iter()
            .filter_map(|adj| {
                if adj.block_a == block_id {
                    Some(adj.block_b.clone())
                } else if adj.block_b == block_id {
                    Some(adj.block_a.clone())
                } else {
                    None
                }
            })
            .collect();

        let blocks = self.blocks.get_cloned();
        blocks
            .iter()
            .filter(|b| adjacent_ids.contains(&b.id))
            .cloned()
            .collect()
    }

    // Signal exposure for reactive UI

    /// Signal for neighborhood ID changes
    pub fn neighborhood_id_signal(
        &self,
    ) -> impl Signal<Item = Option<String>> + Send + Sync + 'static {
        self.neighborhood_id.signal()
    }

    /// Signal for neighborhood name changes
    pub fn neighborhood_name_signal(
        &self,
    ) -> impl Signal<Item = Option<String>> + Send + Sync + 'static {
        self.neighborhood_name.signal()
    }

    /// Signal for blocks changes
    pub fn blocks_signal(
        &self,
    ) -> impl SignalVec<Item = NeighborhoodBlock> + Send + Sync + 'static {
        self.blocks.signal_vec()
    }

    /// Signal for adjacencies changes
    pub fn adjacencies_signal(
        &self,
    ) -> impl SignalVec<Item = BlockAdjacency> + Send + Sync + 'static {
        self.adjacencies.signal_vec()
    }

    /// Signal for position changes
    pub fn position_signal(&self) -> impl Signal<Item = TraversalPosition> + Send + Sync + 'static {
        self.position.signal()
    }

    /// Signal for block count
    pub fn blocks_count_signal(&self) -> impl Signal<Item = usize> + Send + Sync + 'static {
        self.blocks.count_signal()
    }
}

impl Default for NeighborhoodView {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// ReactiveViewModel - Unified View Aggregation
// ============================================================================

/// Unified reactive view model aggregating all views
///
/// Provides cross-view derived state and a unified signal API for the TUI.
/// This enables efficient rendering and reduces code duplication across screens.
#[derive(Clone)]
pub struct ReactiveViewModel {
    /// Welcome view (account status, authority ID)
    pub welcome: WelcomeView,
    /// Chat view (channels, messages, selected channel)
    pub chat: ChatView,
    /// Guardians view (guardian list, threshold info)
    pub guardians: GuardiansView,
    /// Recovery view (session status, approval progress)
    pub recovery: RecoveryView,
    /// Invitations view (pending/accepted invitations)
    pub invitations: InvitationsView,
    /// Block view (current block info, residents, storage)
    pub block: BlockView,
    /// Contacts view (contact list, suggestion policy)
    pub contacts: ContactsView,
    /// Neighborhood view (neighborhood blocks, traversal position)
    pub neighborhood: NeighborhoodView,
}

impl ReactiveViewModel {
    /// Create a new unified view model with all views initialized
    pub fn new() -> Self {
        Self {
            welcome: WelcomeView::new(),
            chat: ChatView::new(),
            guardians: GuardiansView::new(),
            recovery: RecoveryView::new(),
            invitations: InvitationsView::new(),
            block: BlockView::new(),
            contacts: ContactsView::new(),
            neighborhood: NeighborhoodView::new(),
        }
    }

    // ============================================================================
    // Cross-View Derived State
    // ============================================================================

    /// Get total count of pending items requiring user attention
    ///
    /// This includes:
    /// - Pending invitations (inbound only)
    /// - Recovery sessions awaiting approval
    ///
    /// Useful for displaying notification badges in the TUI.
    pub fn pending_notifications_count(&self) -> usize {
        let pending_invitations = self.invitations.pending_inbound().len();

        let recovery_pending = if let RecoveryState::Initiated = self.recovery.get_status().state {
            1
        } else {
            0
        };

        pending_invitations + recovery_pending
    }

    /// Check if any critical action is required
    ///
    /// Returns true if:
    /// - Recovery session has met threshold and needs completion
    /// - There are expired pending invitations
    pub fn has_critical_notifications(&self) -> bool {
        // Check if recovery has met threshold
        let recovery_status = self.recovery.get_status();
        let recovery_ready = matches!(recovery_status.state, RecoveryState::ThresholdMet);

        // For now, just check recovery. Can add invitation expiry checks later.
        recovery_ready
    }

    /// Get summary statistics for the dashboard
    pub fn get_dashboard_stats(&self) -> DashboardStats {
        DashboardStats {
            total_channels: self.chat.get_channels().len(),
            total_guardians: self.guardians.get_guardians().len(),
            pending_invitations: self.invitations.pending_count(),
            block_residents: self.block.get_residents().len(),
            storage_used_percent: {
                let storage = self.block.get_storage();
                if storage.total_bytes > 0 {
                    (storage.used_bytes as f64 / storage.total_bytes as f64 * 100.0) as u8
                } else {
                    0
                }
            },
        }
    }

    /// Check if the account is properly set up
    ///
    /// Returns true if:
    /// - Guardians are configured with valid threshold
    /// - At least one channel exists
    pub fn is_account_ready(&self) -> bool {
        self.guardians.get_threshold().is_some()
    }

    // ============================================================================
    // Advanced Derived State (Filtered, Sorted, Aggregated)
    // ============================================================================

    /// Get active guardians only
    ///
    /// Filters the guardian list to only those with Active status.
    /// Useful for displaying available guardians for recovery operations.
    pub fn get_active_guardians(&self) -> Vec<Guardian> {
        use super::queries::GuardianStatus;
        self.guardians
            .get_guardians()
            .into_iter()
            .filter(|g| matches!(g.status, GuardianStatus::Active))
            .collect()
    }

    /// Get sorted guardians by priority (active first, then by name)
    ///
    /// Returns guardians sorted with active guardians first,
    /// then alphabetically by name within each group.
    pub fn get_sorted_guardians(&self) -> Vec<Guardian> {
        use super::queries::GuardianStatus;
        let mut guardians = self.guardians.get_guardians();
        guardians.sort_by(|a, b| {
            // Active guardians first
            let a_active = matches!(a.status, GuardianStatus::Active);
            let b_active = matches!(b.status, GuardianStatus::Active);
            match (a_active, b_active) {
                (true, false) => std::cmp::Ordering::Less,
                (false, true) => std::cmp::Ordering::Greater,
                _ => a.name.cmp(&b.name), // Then alphabetical by name
            }
        });
        guardians
    }

    /// Get pending inbound invitations only
    ///
    /// Filters invitations to only pending inbound requests
    /// that require user action.
    pub fn get_pending_inbound_invitations(&self) -> Vec<Invitation> {
        self.invitations.pending_inbound()
    }

    /// Get total storage usage across all blocks
    ///
    /// Returns a tuple of (used_bytes, total_bytes, percentage).
    pub fn get_total_storage_usage(&self) -> (u64, u64, f64) {
        let storage = self.block.get_storage();
        let percentage = if storage.total_bytes > 0 {
            (storage.used_bytes as f64 / storage.total_bytes as f64) * 100.0
        } else {
            0.0
        };
        (storage.used_bytes, storage.total_bytes, percentage)
    }

    /// Check if storage is critically low (>90% used)
    pub fn is_storage_critical(&self) -> bool {
        let (_, _, percentage) = self.get_total_storage_usage();
        percentage > 90.0
    }

    /// Get aggregated channel statistics
    ///
    /// Returns statistics about chat channels:
    /// - Total channels
    /// - Channels with unread messages (placeholder - needs message tracking)
    pub fn get_channel_stats(&self) -> ChannelStats {
        let channels = self.chat.get_channels();
        ChannelStats {
            total_channels: channels.len(),
            // Placeholder: In a real implementation, track unread messages per channel
            unread_channels: 0,
        }
    }

    /// Get recovery progress percentage
    ///
    /// Returns 0-100 representing recovery session progress.
    /// Returns 0 if no active recovery session.
    pub fn get_recovery_progress(&self) -> u8 {
        self.recovery.progress_percent() as u8
    }

    /// Check if recovery is ready to complete
    ///
    /// Returns true if threshold has been met and recovery can be finalized.
    pub fn is_recovery_ready(&self) -> bool {
        let status = self.recovery.get_status();
        matches!(status.state, RecoveryState::ThresholdMet)
    }

    /// Get combined notification summary
    ///
    /// Returns a structured summary of all pending notifications
    /// across views for display in a unified notification panel.
    pub fn get_notification_summary(&self) -> NotificationSummary {
        NotificationSummary {
            pending_invitations: self.invitations.pending_count(),
            recovery_awaiting: if matches!(
                self.recovery.get_status().state,
                RecoveryState::Initiated
            ) {
                1
            } else {
                0
            },
            storage_critical: self.is_storage_critical(),
            recovery_ready: self.is_recovery_ready(),
        }
    }

    // ============================================================================
    // Unified Signal API
    // ============================================================================

    /// Get a signal that emits when any view updates
    ///
    /// This is useful for triggering full TUI redraws when any data changes.
    /// Individual screens should use specific view signals for targeted updates.
    pub fn any_view_signal(
        &self,
    ) -> impl futures_signals::signal::Signal<Item = ()> + Send + Sync + 'static {
        use futures_signals::signal::SignalExt;

        // Use chat selection as a proxy "any change" signal until broader aggregation is needed.
        self.chat.selected_channel_signal().map(|_| ())
    }
}

impl Default for ReactiveViewModel {
    fn default() -> Self {
        Self::new()
    }
}

/// Dashboard statistics derived from multiple views
#[derive(Debug, Clone)]
pub struct DashboardStats {
    /// Total number of chat channels
    pub total_channels: usize,
    /// Total number of configured guardians
    pub total_guardians: usize,
    /// Number of pending invitations
    pub pending_invitations: usize,
    /// Number of residents in current block
    pub block_residents: usize,
    /// Storage used percentage (0-100)
    pub storage_used_percent: u8,
}

/// Channel statistics for chat
#[derive(Debug, Clone)]
pub struct ChannelStats {
    /// Total number of channels
    pub total_channels: usize,
    /// Number of channels with unread messages
    pub unread_channels: usize,
}

/// Unified notification summary across all views
#[derive(Debug, Clone)]
pub struct NotificationSummary {
    /// Number of pending invitations
    pub pending_invitations: usize,
    /// Number of recovery sessions awaiting approval
    pub recovery_awaiting: usize,
    /// Whether storage is critically low
    pub storage_critical: bool,
    /// Whether recovery is ready to complete
    pub recovery_ready: bool,
}

impl NotificationSummary {
    /// Get total count of actionable notifications
    pub fn total_actionable(&self) -> usize {
        let mut count = self.pending_invitations + self.recovery_awaiting;
        if self.recovery_ready {
            count += 1;
        }
        count
    }

    /// Check if there are any critical notifications
    pub fn has_critical(&self) -> bool {
        self.storage_critical || self.recovery_ready
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

    #[test]
    fn test_chat_view_creation() {
        let view = ChatView::new();
        assert!(view.get_selected_channel().is_none());
        let channels = view.get_channels();
        assert!(channels.is_empty());
    }

    #[test]
    fn test_chat_view_select_channel() {
        let view = ChatView::new();
        view.select_channel(Some("general".to_string()));
        assert_eq!(view.get_selected_channel(), Some("general".to_string()));
    }

    #[test]
    fn test_chat_view_update_channels() {
        let view = ChatView::new();
        let channels = vec![Channel {
            id: "general".to_string(),
            name: "General".to_string(),
            ..Default::default()
        }];
        view.update_channels(channels);
        assert_eq!(view.get_channels().len(), 1);
    }

    #[test]
    fn test_guardians_view_creation() {
        let view = GuardiansView::new();
        assert!(view.get_threshold().is_none());
        let guardians = view.get_guardians();
        assert!(guardians.is_empty());
    }

    #[test]
    fn test_guardians_view_update_threshold() {
        let view = GuardiansView::new();
        view.update_threshold(2, 3);
        let threshold = view.get_threshold().unwrap();
        assert_eq!(threshold.threshold, 2);
        assert_eq!(threshold.total, 3);
    }

    #[test]
    fn test_recovery_view_creation() {
        let view = RecoveryView::new();
        let status = view.get_status();
        assert_eq!(status.state, RecoveryState::None);
    }

    #[test]
    fn test_recovery_view_progress() {
        let view = RecoveryView::new();
        view.update_status(RecoveryStatus {
            threshold: 2,
            approvals_received: 1,
            ..Default::default()
        });
        assert_eq!(view.progress_percent(), 50);
    }

    #[test]
    fn test_invitations_view_creation() {
        let view = InvitationsView::new();
        let invitations = view.get_invitations();
        assert!(invitations.is_empty());
    }

    #[test]
    fn test_invitations_view_pending_count() {
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
        ]);
        assert_eq!(view.pending_count(), 1);
    }
}
