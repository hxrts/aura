//! # Effect Bridge Implementation
//!
//! Provides the connection between the TUI and Aura's effect system.
//! Handles command dispatch, event subscription, and error recovery.
//!
//! This module has been split for maintainability:
//! - `command_parser`: Command and event types, authorization
//! - `bridge_config`: Configuration types
//! - `effect_dispatch`: Command execution and authorization logic
//! - `frost_helpers`: FROST signing utilities for TreeOps
//! - `bridge`: Core bridge implementation (this file)

use std::sync::Arc;

pub use crate::tui::effects::bridge_config::BridgeConfig;
pub use crate::tui::effects::command_parser::{
    AuraEvent, EffectCommand, EventFilter, EventSubscription,
};
use aura_app::AppCore;
use aura_core::effects::amp::AmpChannelEffects;
use aura_core::effects::time::PhysicalTimeEffects;
use aura_effects::time::PhysicalTimeHandler;
use tokio::sync::{broadcast, mpsc, oneshot, RwLock};

// Import shared types and functions from effect_dispatch module
use super::effect_dispatch::{execute_command, BridgeState};

// Re-export FROST helpers for use by effect_dispatch
pub(super) use super::frost_helpers::{frost_sign_tree_op, frost_sign_tree_op_with_keys};

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
    /// Optional full effect system (provides access to all effect handlers)
    #[allow(dead_code)]
    effect_system: Option<Arc<aura_agent::AuraEffectSystem>>,
    /// Optional AppCore for intent-based state management
    #[allow(dead_code)]
    app_core: Option<Arc<RwLock<AppCore>>>,
}

impl EffectBridge {
    /// Create a new effect bridge with default configuration
    pub fn new() -> Self {
        Self::with_config_time_amp(BridgeConfig::default(), Arc::new(PhysicalTimeHandler), None)
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
        Self::with_config_time_amp_system(config, time_effects, amp_effects, None)
    }

    /// Create a new effect bridge with full configuration including effect system
    pub fn with_config_time_amp_system(
        config: BridgeConfig,
        time_effects: Arc<dyn PhysicalTimeEffects>,
        amp_effects: Option<Arc<dyn AmpChannelEffects + Send + Sync>>,
        effect_system: Option<Arc<aura_agent::AuraEffectSystem>>,
    ) -> Self {
        Self::with_full_config(config, time_effects, amp_effects, effect_system, None)
    }

    /// Create a new effect bridge with full configuration including effect system and AppCore
    pub fn with_full_config(
        config: BridgeConfig,
        time_effects: Arc<dyn PhysicalTimeEffects>,
        amp_effects: Option<Arc<dyn AmpChannelEffects + Send + Sync>>,
        effect_system: Option<Arc<aura_agent::AuraEffectSystem>>,
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
            effect_system: effect_system.clone(),
            app_core: app_core.clone(),
        };

        // Start command consumer loop in background
        tokio::spawn(Self::command_consumer_loop(
            command_rx,
            event_tx,
            bridge.state.clone(),
            config,
            time_effects,
            amp_effects,
            effect_system,
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
        amp_effects: Option<Arc<dyn AmpChannelEffects + Send + Sync>>,
        effect_system: Option<Arc<aura_agent::AuraEffectSystem>>,
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
                effect_system.as_ref(),
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
        effect_system: Option<&Arc<aura_agent::AuraEffectSystem>>,
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
                effect_system,
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
