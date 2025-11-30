//! # Effect Bridge Implementation
//!
//! Provides the connection between the TUI and Aura's effect system.
//! Handles command dispatch, event subscription, and error recovery.

use std::sync::Arc;
use std::time::Duration;

use aura_core::effects::time::PhysicalTimeEffects;
use aura_effects::time::PhysicalTimeHandler;
use tokio::sync::{broadcast, mpsc, oneshot, RwLock};

/// Configuration for the effect bridge
#[derive(Debug, Clone)]
pub struct BridgeConfig {
    /// Maximum pending commands in the queue
    pub command_buffer_size: usize,
    /// Maximum pending events in the broadcast channel
    pub event_buffer_size: usize,
    /// Timeout for command execution
    pub command_timeout: Duration,
    /// Enable automatic retry on transient failures
    pub auto_retry: bool,
    /// Maximum retry attempts
    pub max_retries: u32,
    /// Backoff duration between retries
    pub retry_backoff: Duration,
}

impl Default for BridgeConfig {
    fn default() -> Self {
        Self {
            command_buffer_size: 256,
            event_buffer_size: 1024,
            command_timeout: Duration::from_secs(30),
            auto_retry: true,
            max_retries: 3,
            retry_backoff: Duration::from_millis(100),
        }
    }
}

/// Commands that can be dispatched to the effect system
#[derive(Debug, Clone)]
pub enum EffectCommand {
    // === Recovery Commands ===
    /// Initiate recovery process
    StartRecovery,
    /// Submit guardian approval
    SubmitGuardianApproval {
        /// Guardian ID
        guardian_id: String,
    },
    /// Complete recovery after threshold met
    CompleteRecovery,
    /// Cancel ongoing recovery
    CancelRecovery,

    // === Account Commands ===
    /// Refresh account status
    RefreshAccount,
    /// Create new account (demo)
    CreateAccount {
        /// Account name
        name: String,
    },

    // === Chat Commands ===
    /// Send a message
    SendMessage {
        /// Channel ID
        channel: String,

        /// Message content
        content: String,
    },
    /// Send a direct message to a user
    SendDirectMessage {
        /// Target user
        target: String,

        /// Message content
        content: String,
    },
    /// Send an action/emote
    SendAction {
        /// Channel ID
        channel: String,

        /// Action text
        action: String,
    },
    /// Join a channel
    JoinChannel {
        /// Channel ID
        channel: String,
    },
    /// Leave a channel
    LeaveChannel {
        /// Channel ID
        channel: String,
    },
    /// Update contact suggestion (nickname)
    UpdateNickname {
        /// New nickname
        name: String,
    },
    /// List participants in channel
    ListParticipants {
        /// Channel ID
        channel: String,
    },
    /// Get user information
    GetUserInfo {
        /// Target user
        target: String,
    },
    /// Kick user from channel
    KickUser {
        /// Channel ID
        channel: String,

        /// Target user
        target: String,

        /// Optional reason
        reason: Option<String>,
    },
    /// Ban user from block
    BanUser {
        /// Target user
        target: String,

        /// Optional reason
        reason: Option<String>,
    },
    /// Unban user from block
    UnbanUser {
        /// Target user
        target: String,
    },
    /// Mute user temporarily
    MuteUser {
        /// Target user
        target: String,

        /// Duration in seconds (None = indefinite)
        duration_secs: Option<u64>,
    },
    /// Unmute user
    UnmuteUser {
        /// Target user
        target: String,
    },
    /// Invite user to channel/block
    InviteUser {
        /// Target user
        target: String,
    },

    // === Invitation Commands ===
    /// Accept an invitation
    AcceptInvitation {
        /// Invitation ID
        invitation_id: String,
    },
    /// Decline an invitation
    DeclineInvitation {
        /// Invitation ID
        invitation_id: String,
    },
    /// Set channel topic
    SetTopic {
        /// Channel ID
        channel: String,

        /// Topic text
        text: String,
    },
    /// Pin message
    PinMessage {
        /// Message ID
        message_id: String,
    },
    /// Unpin message
    UnpinMessage {
        /// Message ID
        message_id: String,
    },
    /// Grant steward capabilities
    GrantSteward {
        /// Target user
        target: String,
    },
    /// Revoke steward capabilities
    RevokeSteward {
        /// Target user
        target: String,
    },
    /// Set channel mode
    SetChannelMode {
        /// Channel ID
        channel: String,

        /// Mode flags
        flags: String,
    },

    // === Sync Commands ===
    /// Force sync with peers
    ForceSync,
    /// Request state from specific peer
    RequestState {
        /// Peer ID
        peer_id: String,
    },

    // === General Commands ===
    /// Ping/health check
    Ping,
    /// Shutdown the bridge
    Shutdown,
}

/// Events emitted by the effect system for TUI consumption
#[derive(Debug, Clone)]
pub enum AuraEvent {
    // === Connection Events ===
    /// Connected to the network
    Connected,
    /// Disconnected from the network
    Disconnected {
        /// Disconnection reason
        reason: String,
    },
    /// Reconnecting after failure
    Reconnecting {
        /// Current attempt number
        attempt: u32,

        /// Maximum attempts
        max_attempts: u32,
    },

    // === Recovery Events ===
    /// Recovery process started
    RecoveryStarted {
        /// Session ID
        session_id: String,
    },
    /// Guardian approved recovery
    GuardianApproved {
        /// Guardian ID
        guardian_id: String,
        /// Current approval count
        current: u32,
        /// Required threshold
        threshold: u32,
    },
    /// Recovery threshold met
    ThresholdMet {
        /// Session ID
        session_id: String,
    },
    /// Recovery completed successfully
    RecoveryCompleted {
        /// Session ID
        session_id: String,
    },
    /// Recovery failed
    RecoveryFailed {
        /// Session ID
        session_id: String,

        /// Failure reason
        reason: String,
    },
    /// Recovery cancelled
    RecoveryCancelled {
        /// Session ID
        session_id: String,
    },

    // === Account Events ===
    /// Account state updated
    AccountUpdated {
        /// Authority ID
        authority_id: String,
    },
    /// New device added to account
    DeviceAdded {
        /// Device ID
        device_id: String,
    },
    /// Device removed from account
    DeviceRemoved {
        /// Device ID
        device_id: String,
    },

    // === Chat Events ===
    /// New message received
    MessageReceived {
        /// Channel ID
        channel: String,

        /// Sender ID
        from: String,

        /// Message content
        content: String,

        /// Timestamp
        timestamp: u64,
    },
    /// User joined channel
    UserJoined {
        /// Channel ID
        channel: String,

        /// User ID
        user: String,
    },
    /// User left channel
    UserLeft {
        /// Channel ID
        channel: String,

        /// User ID
        user: String,
    },

    // === Sync Events ===
    /// Sync started
    SyncStarted {
        /// Peer ID
        peer_id: String,
    },
    /// Sync completed
    SyncCompleted {
        /// Peer ID
        peer_id: String,

        /// Number of changes synced
        changes: u32,
    },
    /// Sync failed
    SyncFailed {
        /// Peer ID
        peer_id: String,

        /// Failure reason
        reason: String,
    },

    // === Error Events ===
    /// General error occurred
    Error {
        /// Error code
        code: String,

        /// Error message
        message: String,
    },
    /// Warning (non-fatal)
    Warning {
        /// Warning message
        message: String,
    },

    // === System Events ===
    /// Pong response to ping
    Pong {
        /// Latency in milliseconds
        latency_ms: u64,
    },
    /// Bridge shutting down
    ShuttingDown,
}

/// Filter for subscribing to specific event types
#[derive(Debug, Clone, Default)]
pub struct EventFilter {
    /// Include connection events
    pub connection: bool,
    /// Include recovery events
    pub recovery: bool,
    /// Include account events
    pub account: bool,
    /// Include chat events
    pub chat: bool,
    /// Include sync events
    pub sync: bool,
    /// Include error events
    pub errors: bool,
    /// Include system events
    pub system: bool,
}

impl EventFilter {
    /// Create a filter that accepts all events
    pub fn all() -> Self {
        Self {
            connection: true,
            recovery: true,
            account: true,
            chat: true,
            sync: true,
            errors: true,
            system: true,
        }
    }

    /// Create a filter for connection and error events only
    pub fn essential() -> Self {
        Self {
            connection: true,
            errors: true,
            system: true,
            ..Default::default()
        }
    }

    /// Create a filter for recovery-related events
    pub fn recovery() -> Self {
        Self {
            recovery: true,
            errors: true,
            ..Default::default()
        }
    }

    /// Check if an event matches this filter
    pub fn matches(&self, event: &AuraEvent) -> bool {
        match event {
            AuraEvent::Connected
            | AuraEvent::Disconnected { .. }
            | AuraEvent::Reconnecting { .. } => self.connection,
            AuraEvent::RecoveryStarted { .. }
            | AuraEvent::GuardianApproved { .. }
            | AuraEvent::ThresholdMet { .. }
            | AuraEvent::RecoveryCompleted { .. }
            | AuraEvent::RecoveryFailed { .. }
            | AuraEvent::RecoveryCancelled { .. } => self.recovery,
            AuraEvent::AccountUpdated { .. }
            | AuraEvent::DeviceAdded { .. }
            | AuraEvent::DeviceRemoved { .. } => self.account,
            AuraEvent::MessageReceived { .. }
            | AuraEvent::UserJoined { .. }
            | AuraEvent::UserLeft { .. } => self.chat,
            AuraEvent::SyncStarted { .. }
            | AuraEvent::SyncCompleted { .. }
            | AuraEvent::SyncFailed { .. } => self.sync,
            AuraEvent::Error { .. } | AuraEvent::Warning { .. } => self.errors,
            AuraEvent::Pong { .. } | AuraEvent::ShuttingDown => self.system,
        }
    }
}

/// Subscription handle for receiving filtered events
pub struct EventSubscription {
    pub(crate) receiver: broadcast::Receiver<AuraEvent>,
    pub(crate) filter: EventFilter,
}

impl EventSubscription {
    /// Create a new event subscription
    pub fn new(receiver: broadcast::Receiver<AuraEvent>, filter: EventFilter) -> Self {
        Self { receiver, filter }
    }
}

impl EventSubscription {
    /// Receive the next event that matches the filter
    pub async fn recv(&mut self) -> Option<AuraEvent> {
        loop {
            match self.receiver.recv().await {
                Ok(event) if self.filter.matches(&event) => return Some(event),
                Ok(_) => continue, // Skip non-matching events
                Err(broadcast::error::RecvError::Closed) => return None,
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
            }
        }
    }

    /// Try to receive an event without blocking
    pub fn try_recv(&mut self) -> Option<AuraEvent> {
        loop {
            match self.receiver.try_recv() {
                Ok(event) if self.filter.matches(&event) => return Some(event),
                Ok(_) => continue,
                Err(_) => return None,
            }
        }
    }
}

/// Internal state for the effect bridge
struct BridgeState {
    /// Whether the bridge is connected
    connected: bool,
    /// Number of pending commands
    pending_commands: u32,
    /// Last error message
    last_error: Option<String>,
}

impl Default for BridgeState {
    fn default() -> Self {
        Self {
            connected: false,
            pending_commands: 0,
            last_error: None,
        }
    }
}

/// Bridge connecting TUI to the effect system
pub struct EffectBridge {
    /// Configuration
    config: BridgeConfig,
    /// Command sender
    command_tx: mpsc::Sender<(EffectCommand, Option<oneshot::Sender<Result<(), String>>>)>,
    /// Event broadcaster
    event_tx: broadcast::Sender<AuraEvent>,
    /// Internal state
    state: Arc<RwLock<BridgeState>>,
    /// Time effects for async delays (injected for testability)
    time_effects: Arc<dyn PhysicalTimeEffects>,
}

impl EffectBridge {
    /// Create a new effect bridge with default configuration
    pub fn new() -> Self {
        Self::with_config(BridgeConfig::default())
    }

    /// Create a new effect bridge with custom configuration
    pub fn with_config(config: BridgeConfig) -> Self {
        Self::with_config_and_time(config, Arc::new(PhysicalTimeHandler))
    }

    /// Create a new effect bridge with custom configuration and time effects
    ///
    /// This constructor allows injecting custom time effects for testing
    /// or simulation scenarios where time control is needed.
    pub fn with_config_and_time(
        config: BridgeConfig,
        time_effects: Arc<dyn PhysicalTimeEffects>,
    ) -> Self {
        let (command_tx, command_rx) = mpsc::channel(config.command_buffer_size);
        let (event_tx, _) = broadcast::channel(config.event_buffer_size);

        let bridge = Self {
            config: config.clone(),
            command_tx,
            event_tx: event_tx.clone(),
            state: Arc::new(RwLock::new(BridgeState::default())),
            time_effects: time_effects.clone(),
        };

        // Start command consumer loop in background
        tokio::spawn(Self::command_consumer_loop(
            command_rx,
            event_tx,
            bridge.state.clone(),
            config,
            time_effects,
        ));

        bridge
    }

    /// Subscribe to events with a filter
    pub fn subscribe(&self, filter: EventFilter) -> EventSubscription {
        EventSubscription {
            receiver: self.event_tx.subscribe(),
            filter,
        }
    }

    /// Subscribe to all events
    pub fn subscribe_all(&self) -> EventSubscription {
        self.subscribe(EventFilter::all())
    }

    /// Dispatch a command to the effect system
    pub async fn dispatch(&self, command: EffectCommand) -> Result<(), String> {
        // Update pending count
        {
            let mut state = self.state.write().await;
            state.pending_commands += 1;
        }

        // Send command
        let result = self
            .command_tx
            .send((command, None))
            .await
            .map_err(|_| "Command channel closed".to_string());

        // Update pending count
        {
            let mut state = self.state.write().await;
            state.pending_commands = state.pending_commands.saturating_sub(1);
        }

        result
    }

    /// Dispatch a command and wait for completion
    pub async fn dispatch_and_wait(&self, command: EffectCommand) -> Result<(), String> {
        let (response_tx, response_rx) = oneshot::channel();

        // Update pending count
        {
            let mut state = self.state.write().await;
            state.pending_commands += 1;
        }

        // Send command with response channel
        self.command_tx
            .send((command, Some(response_tx)))
            .await
            .map_err(|_| "Command channel closed".to_string())?;

        // Wait for response with timeout
        let result = tokio::time::timeout(self.config.command_timeout, response_rx)
            .await
            .map_err(|_| "Command timed out".to_string())?
            .map_err(|_| "Response channel closed".to_string())?;

        // Update pending count
        {
            let mut state = self.state.write().await;
            state.pending_commands = state.pending_commands.saturating_sub(1);
        }

        result
    }

    /// Emit an event to all subscribers
    pub fn emit(&self, event: AuraEvent) {
        // Ignore send errors (no subscribers)
        let _ = self.event_tx.send(event);
    }

    /// Check if the bridge is connected
    pub async fn is_connected(&self) -> bool {
        self.state.read().await.connected
    }

    /// Set connection status
    pub async fn set_connected(&self, connected: bool) {
        let mut state = self.state.write().await;
        state.connected = connected;
    }

    /// Get the number of pending commands
    pub async fn pending_commands(&self) -> u32 {
        self.state.read().await.pending_commands
    }

    /// Get the last error message
    pub async fn last_error(&self) -> Option<String> {
        self.state.read().await.last_error.clone()
    }

    /// Set an error state
    pub async fn set_error(&self, error: impl Into<String>) {
        let error = error.into();
        {
            let mut state = self.state.write().await;
            state.last_error = Some(error.clone());
        }
        self.emit(AuraEvent::Error {
            code: "BRIDGE_ERROR".to_string(),
            message: error,
        });
    }

    /// Clear error state
    pub async fn clear_error(&self) {
        let mut state = self.state.write().await;
        state.last_error = None;
    }

    /// Get the configuration
    pub fn config(&self) -> &BridgeConfig {
        &self.config
    }

    /// Command consumer loop (runs in background)
    async fn command_consumer_loop(
        mut command_rx: mpsc::Receiver<(
            EffectCommand,
            Option<oneshot::Sender<Result<(), String>>>,
        )>,
        event_tx: broadcast::Sender<AuraEvent>,
        state: Arc<RwLock<BridgeState>>,
        config: BridgeConfig,
        time_effects: Arc<dyn PhysicalTimeEffects>,
    ) {
        tracing::info!("EffectBridge command consumer loop started");

        while let Some((command, response_tx)) = command_rx.recv().await {
            // Process command with retry logic
            let result =
                Self::process_command_with_retry(&command, &event_tx, &config, &time_effects).await;

            // Send response if requested
            if let Some(tx) = response_tx {
                let _ = tx.send(result.clone());
            }

            // Update state on error
            if let Err(ref e) = result {
                let mut s = state.write().await;
                s.last_error = Some(e.clone());
            }
        }

        tracing::info!("EffectBridge command consumer loop stopped");
    }

    /// Process a command with automatic retry on transient failures
    async fn process_command_with_retry(
        command: &EffectCommand,
        event_tx: &broadcast::Sender<AuraEvent>,
        config: &BridgeConfig,
        time_effects: &Arc<dyn PhysicalTimeEffects>,
    ) -> Result<(), String> {
        let mut attempts = 0;
        let mut last_error = None;

        while attempts <= config.max_retries {
            match Self::execute_command(command, event_tx).await {
                Ok(()) => return Ok(()),
                Err(e)
                    if Self::is_transient_error(&e)
                        && config.auto_retry
                        && attempts < config.max_retries =>
                {
                    attempts += 1;
                    last_error = Some(e.clone());

                    tracing::warn!(
                        "Transient error executing {:?}, attempt {}/{}: {}",
                        command,
                        attempts,
                        config.max_retries,
                        e
                    );

                    // Exponential backoff using injected time effects
                    let backoff = config.retry_backoff * (2_u32.pow(attempts - 1));
                    let _ = time_effects.sleep_ms(backoff.as_millis() as u64).await;
                }
                Err(e) => {
                    // Non-transient error or max retries reached
                    return Err(e);
                }
            }
        }

        Err(last_error.unwrap_or_else(|| "Max retries exceeded".to_string()))
    }

    /// Check if an error is transient and can be retried
    fn is_transient_error(error: &str) -> bool {
        // Common transient error patterns
        error.contains("timeout")
            || error.contains("connection")
            || error.contains("network")
            || error.contains("unavailable")
            || error.contains("busy")
    }

    /// Execute a single command
    ///
    /// # Current Status
    ///
    /// This function currently contains stub implementations that emit mock events.
    ///
    /// # Integration with Actual Effect Handlers
    ///
    /// To wire this to actual Aura effect handlers, you would:
    ///
    /// 1. **Add EffectContext to EffectBridge**:
    ///    - Modify `EffectBridge` to hold an `Arc<EffectContext>`
    ///    - The context provides access to all effect handlers (ChatEffects, RecoveryEffects, etc.)
    ///
    /// 2. **Replace stub implementations** with actual effect calls:
    ///    ```rust
    ///    // Example for SendMessage:
    ///    EffectCommand::SendMessage { channel, content } => {
    ///        context.execute(|effects: &impl ChatEffects| async move {
    ///            effects.send_message(&channel, &content).await
    ///        }).await?;
    ///        Ok(())
    ///    }
    ///    ```
    ///
    /// 3. **Wire effect handlers to emit events**:
    ///    - Effect handlers should emit AuraEvent variants through the bridge
    ///    - Use the event_tx broadcast channel to notify all subscribers
    ///
    /// 4. **Handle errors properly**:
    ///    - Convert effect handler errors to String for return type
    ///    - Emit AuraEvent::Error for failures
    ///    - Distinguish transient vs permanent failures for retry logic
    ///
    /// 5. **Add authorization checks**:
    ///    - Check Biscuit capabilities before executing commands
    ///    - Integrate with CapGuard from aura-protocol
    ///
    /// See docs/106_effect_system_and_runtime.md for effect system architecture.
    /// See crates/aura-protocol/src/guards/ for capability checking.
    async fn execute_command(
        command: &EffectCommand,
        event_tx: &broadcast::Sender<AuraEvent>,
    ) -> Result<(), String> {
        tracing::debug!("Executing command: {:?}", command);

        // STUB IMPLEMENTATIONS BELOW
        // Replace these with actual effect handler calls when integrating with runtime
        match command {
            // Recovery commands
            EffectCommand::StartRecovery => {
                let _ = event_tx.send(AuraEvent::RecoveryStarted {
                    session_id: uuid::Uuid::new_v4().to_string(),
                });
                Ok(())
            }

            EffectCommand::SubmitGuardianApproval { guardian_id } => {
                let _ = event_tx.send(AuraEvent::GuardianApproved {
                    guardian_id: guardian_id.clone(),
                    current: 1,
                    threshold: 2,
                });
                Ok(())
            }

            EffectCommand::CompleteRecovery => {
                let _ = event_tx.send(AuraEvent::RecoveryCompleted {
                    session_id: uuid::Uuid::new_v4().to_string(),
                });
                Ok(())
            }

            EffectCommand::CancelRecovery => {
                let _ = event_tx.send(AuraEvent::RecoveryCancelled {
                    session_id: uuid::Uuid::new_v4().to_string(),
                });
                Ok(())
            }

            // Chat commands
            EffectCommand::SendMessage { channel, content } => {
                let _ = event_tx.send(AuraEvent::MessageReceived {
                    channel: channel.clone(),
                    from: "self".to_string(),
                    content: content.clone(),
                    timestamp: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_secs(),
                });
                Ok(())
            }

            EffectCommand::SendDirectMessage { target, content } => {
                tracing::info!("Sending DM to {}: {}", target, content);
                Ok(())
            }

            EffectCommand::SendAction { channel, action } => {
                let _ = event_tx.send(AuraEvent::MessageReceived {
                    channel: channel.clone(),
                    from: "self".to_string(),
                    content: format!("* {}", action),
                    timestamp: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_secs(),
                });
                Ok(())
            }

            EffectCommand::JoinChannel { channel } => {
                let _ = event_tx.send(AuraEvent::UserJoined {
                    channel: channel.clone(),
                    user: "self".to_string(),
                });
                Ok(())
            }

            EffectCommand::LeaveChannel { channel } => {
                let _ = event_tx.send(AuraEvent::UserLeft {
                    channel: channel.clone(),
                    user: "self".to_string(),
                });
                Ok(())
            }

            // User/moderation commands (stub)
            EffectCommand::UpdateNickname { name } => {
                tracing::info!("Updated nickname to: {}", name);
                Ok(())
            }

            // Invitation commands
            EffectCommand::AcceptInvitation { invitation_id } => {
                tracing::info!("Accepted invitation: {}", invitation_id);
                Ok(())
            }

            EffectCommand::DeclineInvitation { invitation_id } => {
                tracing::info!("Declined invitation: {}", invitation_id);
                Ok(())
            }

            EffectCommand::ListParticipants { .. }
            | EffectCommand::GetUserInfo { .. }
            | EffectCommand::KickUser { .. }
            | EffectCommand::BanUser { .. }
            | EffectCommand::UnbanUser { .. }
            | EffectCommand::MuteUser { .. }
            | EffectCommand::UnmuteUser { .. }
            | EffectCommand::InviteUser { .. }
            | EffectCommand::SetTopic { .. }
            | EffectCommand::PinMessage { .. }
            | EffectCommand::UnpinMessage { .. }
            | EffectCommand::GrantSteward { .. }
            | EffectCommand::RevokeSteward { .. }
            | EffectCommand::SetChannelMode { .. } => {
                tracing::info!("Command stub: {:?}", command);
                Ok(())
            }

            // Account commands
            EffectCommand::RefreshAccount => {
                let _ = event_tx.send(AuraEvent::AccountUpdated {
                    authority_id: uuid::Uuid::new_v4().to_string(),
                });
                Ok(())
            }

            EffectCommand::CreateAccount { .. } => {
                tracing::info!("Account creation stub");
                Ok(())
            }

            // Sync commands
            EffectCommand::ForceSync => {
                let peer_id = uuid::Uuid::new_v4().to_string();
                let _ = event_tx.send(AuraEvent::SyncStarted {
                    peer_id: peer_id.clone(),
                });
                let _ = event_tx.send(AuraEvent::SyncCompleted {
                    peer_id,
                    changes: 0,
                });
                Ok(())
            }

            EffectCommand::RequestState { peer_id } => {
                let _ = event_tx.send(AuraEvent::SyncStarted {
                    peer_id: peer_id.clone(),
                });
                Ok(())
            }

            // System commands
            EffectCommand::Ping => {
                let _ = event_tx.send(AuraEvent::Pong { latency_ms: 10 });
                Ok(())
            }

            EffectCommand::Shutdown => {
                let _ = event_tx.send(AuraEvent::ShuttingDown);
                Ok(())
            }
        }
    }
}

impl Default for EffectBridge {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bridge_config_default() {
        let config = BridgeConfig::default();
        assert_eq!(config.command_buffer_size, 256);
        assert_eq!(config.event_buffer_size, 1024);
        assert!(config.auto_retry);
        assert_eq!(config.max_retries, 3);
    }

    #[test]
    fn test_event_filter_all() {
        let filter = EventFilter::all();
        assert!(filter.connection);
        assert!(filter.recovery);
        assert!(filter.account);
        assert!(filter.chat);
        assert!(filter.sync);
        assert!(filter.errors);
        assert!(filter.system);
    }

    #[test]
    fn test_event_filter_essential() {
        let filter = EventFilter::essential();
        assert!(filter.connection);
        assert!(!filter.recovery);
        assert!(!filter.account);
        assert!(!filter.chat);
        assert!(!filter.sync);
        assert!(filter.errors);
        assert!(filter.system);
    }

    #[test]
    fn test_event_filter_matches() {
        let recovery_filter = EventFilter::recovery();

        assert!(recovery_filter.matches(&AuraEvent::RecoveryStarted {
            session_id: "test".to_string()
        }));

        assert!(recovery_filter.matches(&AuraEvent::Error {
            code: "ERR".to_string(),
            message: "test".to_string()
        }));

        assert!(!recovery_filter.matches(&AuraEvent::Connected));

        assert!(!recovery_filter.matches(&AuraEvent::MessageReceived {
            channel: "test".to_string(),
            from: "user".to_string(),
            content: "hi".to_string(),
            timestamp: 0,
        }));
    }

    #[tokio::test]
    async fn test_bridge_creation() {
        let bridge = EffectBridge::new();
        assert!(!bridge.is_connected().await);
        assert_eq!(bridge.pending_commands().await, 0);
        assert!(bridge.last_error().await.is_none());
    }

    #[tokio::test]
    async fn test_bridge_connection_state() {
        let bridge = EffectBridge::new();

        bridge.set_connected(true).await;
        assert!(bridge.is_connected().await);

        bridge.set_connected(false).await;
        assert!(!bridge.is_connected().await);
    }

    #[tokio::test]
    async fn test_bridge_error_state() {
        let bridge = EffectBridge::new();

        bridge.set_error("Test error").await;
        assert_eq!(bridge.last_error().await, Some("Test error".to_string()));

        bridge.clear_error().await;
        assert!(bridge.last_error().await.is_none());
    }

    #[tokio::test]
    async fn test_bridge_emit_event() {
        let bridge = EffectBridge::new();
        let mut sub = bridge.subscribe(EventFilter::all());

        bridge.emit(AuraEvent::Connected);

        // Use try_recv to check if event was received
        // Note: In real usage, recv() would be used in an async context
        let event = sub.try_recv();
        assert!(matches!(event, Some(AuraEvent::Connected)));
    }

    #[tokio::test]
    async fn test_subscription_filter() {
        let bridge = EffectBridge::new();
        let mut recovery_sub = bridge.subscribe(EventFilter::recovery());

        // Emit non-matching event
        bridge.emit(AuraEvent::Connected);

        // Emit matching event
        bridge.emit(AuraEvent::RecoveryStarted {
            session_id: "test".to_string(),
        });

        // Should only receive the recovery event
        let event = recovery_sub.try_recv();
        assert!(matches!(event, Some(AuraEvent::RecoveryStarted { .. })));
    }
}
