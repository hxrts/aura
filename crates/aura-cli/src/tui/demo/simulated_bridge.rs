//! # Simulated Bridge
//!
//! A mock implementation of the effect bridge for demo mode.
//! Provides the same public interface as EffectBridge but with
//! simulated responses and scheduled guardian approvals.

use std::sync::Arc;

use aura_core::effects::time::PhysicalTimeEffects;
use aura_effects::time::PhysicalTimeHandler;
use tokio::sync::{broadcast, RwLock};

use super::mock_store::MockStore;
use super::DemoScenario;
use crate::tui::effects::{AuraEvent, EffectCommand, EventFilter, EventSubscription};
use crate::tui::reactive::{Message, RecoveryState};

/// Simulated bridge for demo mode
///
/// Implements the same interface as EffectBridge but returns mock data
/// and schedules simulated events like guardian approvals.
pub struct SimulatedBridge {
    /// Mock data store
    store: Arc<MockStore>,
    /// Event broadcaster
    event_tx: broadcast::Sender<AuraEvent>,
    /// Demo scenario configuration
    scenario: DemoScenario,
    /// Whether the bridge is "connected"
    connected: Arc<RwLock<bool>>,
    /// Last error message
    last_error: Arc<RwLock<Option<String>>>,
    /// Pending command count
    pending_commands: Arc<RwLock<u32>>,
    /// Time effects for async delays (injected for testability)
    time_effects: Arc<dyn PhysicalTimeEffects>,
}

impl SimulatedBridge {
    /// Create a new simulated bridge with default scenario
    pub fn new() -> Self {
        Self::with_scenario(DemoScenario::default())
    }

    /// Create a new simulated bridge with a specific scenario
    pub fn with_scenario(scenario: DemoScenario) -> Self {
        Self::with_scenario_and_time(scenario, Arc::new(PhysicalTimeHandler))
    }

    /// Create a new simulated bridge with scenario and custom time effects
    ///
    /// This constructor allows injecting custom time effects for testing
    /// or simulation scenarios where time control is needed.
    pub fn with_scenario_and_time(
        scenario: DemoScenario,
        time_effects: Arc<dyn PhysicalTimeEffects>,
    ) -> Self {
        let (event_tx, _) = broadcast::channel(1024);
        let store = Arc::new(MockStore::new());

        Self {
            store,
            event_tx,
            scenario,
            connected: Arc::new(RwLock::new(true)),
            last_error: Arc::new(RwLock::new(None)),
            pending_commands: Arc::new(RwLock::new(0)),
            time_effects,
        }
    }

    /// Get a reference to the mock store
    pub fn store(&self) -> &MockStore {
        &self.store
    }

    /// Initialize the demo data
    pub async fn initialize(&self) {
        self.store.load_demo_data().await;
    }

    /// Subscribe to events with a filter
    pub fn subscribe(&self, filter: EventFilter) -> EventSubscription {
        EventSubscription::new(self.event_tx.subscribe(), filter)
    }

    /// Subscribe to all events
    pub fn subscribe_all(&self) -> EventSubscription {
        self.subscribe(EventFilter::all())
    }

    /// Dispatch a command
    pub async fn dispatch(&self, command: EffectCommand) -> Result<(), String> {
        // Update pending count
        {
            let mut pending = self.pending_commands.write().await;
            *pending += 1;
        }

        // Execute command
        let result = self.execute_command(command).await;

        // Update pending count
        {
            let mut pending = self.pending_commands.write().await;
            *pending = pending.saturating_sub(1);
        }

        result
    }

    /// Dispatch a command and wait for completion (same as dispatch for demo)
    pub async fn dispatch_and_wait(&self, command: EffectCommand) -> Result<(), String> {
        self.dispatch(command).await
    }

    /// Emit an event to all subscribers
    pub fn emit(&self, event: AuraEvent) {
        let _ = self.event_tx.send(event);
    }

    /// Check if the bridge is connected
    pub async fn is_connected(&self) -> bool {
        *self.connected.read().await
    }

    /// Set connection status
    pub async fn set_connected(&self, connected: bool) {
        *self.connected.write().await = connected;
    }

    /// Get the number of pending commands
    pub async fn pending_commands(&self) -> u32 {
        *self.pending_commands.read().await
    }

    /// Get the last error message
    pub async fn last_error(&self) -> Option<String> {
        self.last_error.read().await.clone()
    }

    /// Set an error state
    pub async fn set_error(&self, error: impl Into<String>) {
        let error = error.into();
        *self.last_error.write().await = Some(error.clone());
        self.emit(AuraEvent::Error {
            code: "DEMO_ERROR".to_string(),
            message: error,
        });
    }

    /// Clear error state
    pub async fn clear_error(&self) {
        *self.last_error.write().await = None;
    }

    /// Get the current scenario
    pub fn scenario(&self) -> DemoScenario {
        self.scenario
    }

    /// Execute a command with demo behavior
    async fn execute_command(&self, command: EffectCommand) -> Result<(), String> {
        tracing::debug!("SimulatedBridge executing: {:?}", command);

        match command {
            // === Recovery Commands ===
            EffectCommand::StartRecovery => self.handle_start_recovery().await,

            EffectCommand::CancelRecovery => {
                self.store.cancel_recovery().await;
                self.emit(AuraEvent::RecoveryCancelled {
                    session_id: "demo_session".to_string(),
                });
                Ok(())
            }

            EffectCommand::CompleteRecovery => {
                self.store.complete_recovery().await;
                self.emit(AuraEvent::RecoveryCompleted {
                    session_id: "demo_session".to_string(),
                });
                Ok(())
            }

            EffectCommand::SubmitGuardianApproval { guardian_id } => {
                // Find guardian name
                let guardians = self.store.get_guardians().await;
                let guardian_name = guardians
                    .iter()
                    .find(|g| g.authority_id == guardian_id)
                    .map(|g| g.name.clone())
                    .unwrap_or_else(|| "Unknown".to_string());

                self.store.add_approval(&guardian_id, &guardian_name).await;

                let recovery = self.store.get_recovery().await;
                self.emit(AuraEvent::GuardianApproved {
                    guardian_id,
                    current: recovery.approvals_received,
                    threshold: recovery.threshold,
                });

                if recovery.state == RecoveryState::Completed {
                    self.emit(AuraEvent::ThresholdMet {
                        session_id: recovery.session_id.unwrap_or_default(),
                    });
                }

                Ok(())
            }

            // === Chat Commands ===
            EffectCommand::SendMessage { channel, content } => {
                let message = Message {
                    id: format!("msg_{}", now_millis()),
                    channel_id: channel.clone(),
                    sender_id: self.store.authority_id.clone(),
                    sender_name: self.store.user_name.clone(),
                    content: content.clone(),
                    timestamp: now_millis(),
                    read: true,
                    is_own: true,
                    reply_to: None,
                };

                self.store.add_message(&channel, message).await;

                self.emit(AuraEvent::MessageReceived {
                    channel,
                    from: self.store.user_name.clone(),
                    content,
                    timestamp: now_millis() / 1000,
                });

                Ok(())
            }

            EffectCommand::SendDirectMessage { target, content } => {
                tracing::info!("Demo: Sending DM to {}: {}", target, content);
                Ok(())
            }

            EffectCommand::SendAction { channel, action } => {
                let message = Message {
                    id: format!("msg_{}", now_millis()),
                    channel_id: channel.clone(),
                    sender_id: self.store.authority_id.clone(),
                    sender_name: self.store.user_name.clone(),
                    content: format!("* {} {}", self.store.user_name, action),
                    timestamp: now_millis(),
                    read: true,
                    is_own: true,
                    reply_to: None,
                };

                self.store.add_message(&channel, message).await;
                Ok(())
            }

            EffectCommand::JoinChannel { channel } => {
                self.emit(AuraEvent::UserJoined {
                    channel,
                    user: self.store.user_name.clone(),
                });
                Ok(())
            }

            EffectCommand::LeaveChannel { channel } => {
                self.emit(AuraEvent::UserLeft {
                    channel,
                    user: self.store.user_name.clone(),
                });
                Ok(())
            }

            // === Invitation Commands ===
            EffectCommand::AcceptInvitation { invitation_id } => {
                tracing::info!("Demo: Accepted invitation {}", invitation_id);
                // In a real implementation, update the invitation status in store
                Ok(())
            }

            EffectCommand::DeclineInvitation { invitation_id } => {
                tracing::info!("Demo: Declined invitation {}", invitation_id);
                Ok(())
            }

            // === Account Commands ===
            EffectCommand::RefreshAccount => {
                self.emit(AuraEvent::AccountUpdated {
                    authority_id: self.store.authority_id.clone(),
                });
                Ok(())
            }

            EffectCommand::CreateAccount { name } => {
                tracing::info!("Demo: Creating account with name {}", name);
                Ok(())
            }

            // === Sync Commands ===
            EffectCommand::ForceSync => {
                let peer_id = "demo_peer".to_string();
                self.emit(AuraEvent::SyncStarted {
                    peer_id: peer_id.clone(),
                });

                // Simulate sync completion after a short delay
                let event_tx = self.event_tx.clone();
                let time_effects = self.time_effects.clone();
                tokio::spawn(async move {
                    let _ = time_effects.sleep_ms(500).await;
                    let _ = event_tx.send(AuraEvent::SyncCompleted {
                        peer_id,
                        changes: 0,
                    });
                });

                Ok(())
            }

            EffectCommand::RequestState { peer_id } => {
                self.emit(AuraEvent::SyncStarted { peer_id });
                Ok(())
            }

            // === System Commands ===
            EffectCommand::Ping => {
                self.emit(AuraEvent::Pong { latency_ms: 5 });
                Ok(())
            }

            EffectCommand::Shutdown => {
                self.emit(AuraEvent::ShuttingDown);
                Ok(())
            }

            // === Other commands routed through the simulated bridge ===
            EffectCommand::UpdateNickname { name } => {
                tracing::info!("Demo: Updated nickname to {}", name);
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
                tracing::debug!("Demo command stub: {:?}", command);
                Ok(())
            }
        }
    }

    /// Handle recovery start with scenario-based behavior
    async fn handle_start_recovery(&self) -> Result<(), String> {
        self.store.start_recovery().await;

        let recovery = self.store.get_recovery().await;
        self.emit(AuraEvent::RecoveryStarted {
            session_id: recovery.session_id.clone().unwrap_or_default(),
        });

        // Schedule guardian responses based on scenario
        if self.scenario.auto_advance() {
            self.schedule_guardian_responses().await;
        }

        Ok(())
    }

    /// Schedule automatic guardian approval responses
    async fn schedule_guardian_responses(&self) {
        let (delay_1, delay_2) = self.scenario.guardian_delays();
        let guardians = self.store.get_guardians().await;

        if guardians.is_empty() {
            return;
        }

        // Schedule first guardian response
        if let Some(guardian) = guardians.first() {
            let store = self.store.clone();
            let event_tx = self.event_tx.clone();
            let guardian_id = guardian.authority_id.clone();
            let guardian_name = guardian.name.clone();
            let scenario = self.scenario;
            let time_effects = self.time_effects.clone();

            tokio::spawn(async move {
                let _ = time_effects.sleep_ms(delay_1).await;

                store.add_approval(&guardian_id, &guardian_name).await;
                let recovery = store.get_recovery().await;

                let _ = event_tx.send(AuraEvent::GuardianApproved {
                    guardian_id,
                    current: recovery.approvals_received,
                    threshold: recovery.threshold,
                });

                // Check if threshold is met
                if recovery.state == RecoveryState::Completed {
                    let _ = event_tx.send(AuraEvent::ThresholdMet {
                        session_id: recovery.session_id.unwrap_or_default(),
                    });
                    let _ = event_tx.send(AuraEvent::RecoveryCompleted {
                        session_id: "demo_session".to_string(),
                    });
                }

                // Handle failed recovery scenario
                if matches!(scenario, DemoScenario::FailedRecovery) {
                    let _ = time_effects.sleep_ms(1000).await;
                    let _ = event_tx.send(AuraEvent::RecoveryFailed {
                        session_id: "demo_session".to_string(),
                        reason: "Simulated failure for demo".to_string(),
                    });
                }
            });
        }

        // Schedule second guardian response
        if let Some(guardian) = guardians.get(1) {
            let store = self.store.clone();
            let event_tx = self.event_tx.clone();
            let guardian_id = guardian.authority_id.clone();
            let guardian_name = guardian.name.clone();
            let time_effects = self.time_effects.clone();

            tokio::spawn(async move {
                let _ = time_effects.sleep_ms(delay_2).await;

                store.add_approval(&guardian_id, &guardian_name).await;
                let recovery = store.get_recovery().await;

                let _ = event_tx.send(AuraEvent::GuardianApproved {
                    guardian_id,
                    current: recovery.approvals_received,
                    threshold: recovery.threshold,
                });

                // Check if threshold is met
                if recovery.state == RecoveryState::Completed {
                    let _ = event_tx.send(AuraEvent::ThresholdMet {
                        session_id: recovery.session_id.unwrap_or_default(),
                    });
                    let _ = event_tx.send(AuraEvent::RecoveryCompleted {
                        session_id: "demo_session".to_string(),
                    });
                }
            });
        }
    }
}

impl Default for SimulatedBridge {
    fn default() -> Self {
        Self::new()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_simulated_bridge_creation() {
        let bridge = SimulatedBridge::new();
        assert!(bridge.is_connected().await);
        assert_eq!(bridge.pending_commands().await, 0);
        assert!(bridge.last_error().await.is_none());
    }

    #[tokio::test]
    async fn test_simulated_bridge_with_scenario() {
        let bridge = SimulatedBridge::with_scenario(DemoScenario::SlowGuardian);
        assert_eq!(bridge.scenario(), DemoScenario::SlowGuardian);
    }

    #[tokio::test]
    async fn test_dispatch_ping() {
        let bridge = SimulatedBridge::new();
        let mut sub = bridge.subscribe_all();

        let result = bridge.dispatch(EffectCommand::Ping).await;
        assert!(result.is_ok());

        let event = sub.try_recv();
        assert!(matches!(event, Some(AuraEvent::Pong { .. })));
    }

    #[tokio::test]
    async fn test_dispatch_send_message() {
        let bridge = SimulatedBridge::new();
        bridge.initialize().await;

        let mut sub = bridge.subscribe_all();

        let result = bridge
            .dispatch(EffectCommand::SendMessage {
                channel: "general".to_string(),
                content: "Hello!".to_string(),
            })
            .await;
        assert!(result.is_ok());

        let event = sub.try_recv();
        assert!(matches!(event, Some(AuraEvent::MessageReceived { .. })));

        // Verify message was added to store
        let messages = bridge.store().get_messages("general").await;
        assert!(!messages.is_empty());
    }

    #[tokio::test]
    async fn test_recovery_flow() {
        let bridge = SimulatedBridge::with_scenario(DemoScenario::Interactive);
        bridge.initialize().await;

        let mut sub = bridge.subscribe_all();

        // Start recovery
        let result = bridge.dispatch(EffectCommand::StartRecovery).await;
        assert!(result.is_ok());

        // Should receive RecoveryStarted event
        let event = sub.try_recv();
        assert!(matches!(event, Some(AuraEvent::RecoveryStarted { .. })));

        // Verify store state
        let recovery = bridge.store().get_recovery().await;
        assert_eq!(recovery.state, RecoveryState::Initiated);
    }

    #[tokio::test]
    async fn test_error_state() {
        let bridge = SimulatedBridge::new();

        bridge.set_error("Test error").await;
        assert_eq!(bridge.last_error().await, Some("Test error".to_string()));

        bridge.clear_error().await;
        assert!(bridge.last_error().await.is_none());
    }
}
