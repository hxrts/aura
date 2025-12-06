//! # Effect Bridge Implementation
//!
//! Provides the connection between the TUI and Aura's effect system.
//! Handles command dispatch, event subscription, and error recovery.
//!
//! This module has been split for maintainability:
//! - `command_parser`: Command and event types, authorization
//! - `bridge_config`: Configuration types
//! - `effect_dispatch`: Command execution and authorization logic
//! - `bridge`: Core bridge implementation (this file)

use std::sync::Arc;

pub use crate::tui::effects::bridge_config::BridgeConfig;
pub use crate::tui::effects::command_parser::{
    AuraEvent, EffectCommand, EventFilter, EventSubscription,
};
use aura_agent::AuraAgent;
use aura_app::AppCore;
use aura_core::effects::amp::AmpChannelEffects;
use aura_core::effects::time::PhysicalTimeEffects;
use aura_effects::time::PhysicalTimeHandler;
use tokio::sync::{broadcast, mpsc, oneshot, RwLock};

// Import shared types and functions from effect_dispatch module
use super::effect_dispatch::{execute_command, BridgeState};

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
    #[allow(dead_code)]
    time_effects: Arc<dyn PhysicalTimeEffects>,
    /// Optional agent for effect system access (dependency inversion pattern)
    #[allow(dead_code)]
    agent: Option<Arc<AuraAgent>>,
    /// Optional AppCore for intent-based state management
    #[allow(dead_code)]
    app_core: Option<Arc<RwLock<AppCore>>>,
}

impl EffectBridge {
    /// Create a new effect bridge with default configuration
    pub fn new() -> Self {
        Self::with_config_time_amp(BridgeConfig::default(), Arc::new(PhysicalTimeHandler), None)
    }

    /// Create a new effect bridge with AppCore integration
    ///
    /// This is the recommended constructor for production use, enabling
    /// the full signal-based reactive architecture via AppCore's ViewState.
    ///
    /// Note: This creates a bridge in demo mode without agent. For full
    /// functionality including effect system access, use `with_agent_and_app_core`.
    pub fn with_app_core(app_core: Arc<RwLock<AppCore>>) -> Self {
        Self::with_complete_config(
            BridgeConfig::default(),
            Arc::new(PhysicalTimeHandler),
            None,
            None,
            Some(app_core),
        )
    }

    /// Create a new effect bridge with agent and AppCore
    ///
    /// This is the full production constructor providing both:
    /// - Agent for effect system and service access
    /// - AppCore for intent-based state management
    pub fn with_agent_and_app_core(agent: Arc<AuraAgent>, app_core: Arc<RwLock<AppCore>>) -> Self {
        Self::with_complete_config(
            BridgeConfig::default(),
            Arc::new(PhysicalTimeHandler),
            None,
            Some(agent),
            Some(app_core),
        )
    }

    /// Create a new effect bridge with custom configuration
    pub fn with_config(config: BridgeConfig) -> Self {
        Self::with_config_time_amp(config, Arc::new(PhysicalTimeHandler), None)
    }

    /// Create a new effect bridge with custom configuration and time effects
    ///
    /// This constructor allows injecting custom time effects for testing
    /// or simulation scenarios where time control is needed.
    pub fn with_config_and_time(
        config: BridgeConfig,
        time_effects: Arc<dyn PhysicalTimeEffects>,
    ) -> Self {
        Self::with_config_time_amp(config, time_effects, None)
    }

    /// Create a new effect bridge with config, time effects, and optional amp effects
    pub fn with_config_time_amp(
        config: BridgeConfig,
        time_effects: Arc<dyn PhysicalTimeEffects>,
        amp_effects: Option<Arc<dyn AmpChannelEffects + Send + Sync>>,
    ) -> Self {
        Self::with_complete_config(config, time_effects, amp_effects, None, None)
    }

    /// Create a new effect bridge with complete configuration
    ///
    /// This constructor provides full control over all bridge components:
    /// - Agent for effect system access (dependency inversion pattern)
    /// - AppCore for intent-based state management
    pub fn with_complete_config(
        config: BridgeConfig,
        time_effects: Arc<dyn PhysicalTimeEffects>,
        amp_effects: Option<Arc<dyn AmpChannelEffects + Send + Sync>>,
        agent: Option<Arc<AuraAgent>>,
        app_core: Option<Arc<RwLock<AppCore>>>,
    ) -> Self {
        let (command_tx, command_rx) = mpsc::channel(config.command_buffer_size);
        let (event_tx, _) = broadcast::channel(config.event_buffer_size);

        let bridge = Self {
            config: config.clone(),
            command_tx,
            event_tx: event_tx.clone(),
            state: Arc::new(RwLock::new(BridgeState::default())),
            time_effects: time_effects.clone(),
            agent: agent.clone(),
            app_core: app_core.clone(),
        };

        // Start command consumer loop in background
        // Agent is passed separately for effect system access (dependency inversion)
        tokio::spawn(Self::command_consumer_loop(
            command_rx,
            event_tx,
            bridge.state.clone(),
            config,
            time_effects,
            amp_effects,
            agent,
            app_core,
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
        self.dispatch_and_wait(command).await
    }

    /// Dispatch a command and wait for completion
    pub async fn dispatch_and_wait(&self, command: EffectCommand) -> Result<(), String> {
        // Update pending count
        {
            let mut state = self.state.write().await;
            state.pending_commands += 1;
        }

        // Send command with response channel
        let (response_tx, response_rx) = oneshot::channel();
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

    /// Check if a sync operation is in progress
    pub async fn is_syncing(&self) -> bool {
        self.state.read().await.sync_in_progress
    }

    /// Get the last sync timestamp (ms since epoch)
    pub async fn last_sync_time(&self) -> Option<u64> {
        self.state.read().await.last_sync_time
    }

    /// Get the number of known peers
    pub async fn known_peers_count(&self) -> usize {
        self.state.read().await.known_peers.len()
    }

    /// Get the configuration
    pub fn config(&self) -> &BridgeConfig {
        &self.config
    }

    /// Get a clone of the event sender for external event injection
    ///
    /// This is useful for demo mode where external agents (Alice, Charlie)
    /// need to emit events into Bob's TUI.
    pub fn event_sender(&self) -> broadcast::Sender<AuraEvent> {
        self.event_tx.clone()
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
        amp_effects: Option<Arc<dyn AmpChannelEffects + Send + Sync>>,
        agent: Option<Arc<AuraAgent>>,
        app_core: Option<Arc<RwLock<AppCore>>>,
    ) {
        tracing::info!("EffectBridge command consumer loop started");

        while let Some((command, response_tx)) = command_rx.recv().await {
            // Process command with retry logic
            let result = Self::process_command_with_retry(
                &command,
                &event_tx,
                &state,
                &config,
                &time_effects,
                amp_effects.as_deref(),
                agent.as_ref(),
                app_core.as_ref(),
            )
            .await;

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
        state: &Arc<RwLock<BridgeState>>,
        config: &BridgeConfig,
        time_effects: &Arc<dyn PhysicalTimeEffects>,
        amp_effects: Option<&(dyn AmpChannelEffects + Send + Sync)>,
        agent: Option<&Arc<AuraAgent>>,
        app_core: Option<&Arc<RwLock<AppCore>>>,
    ) -> Result<(), String> {
        let mut attempts = 0;
        let mut last_error = None;

        while attempts <= config.max_retries {
            match execute_command(
                command,
                event_tx,
                state,
                time_effects,
                amp_effects,
                agent,
                app_core,
            )
            .await
            {
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
}

impl Default for EffectBridge {
    fn default() -> Self {
        Self::new()
    }
}
