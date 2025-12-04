//! TUI Demo End-to-End Tests
//!
//! Tests the TUI demo mode using the SimulatedBridge backend.
//! These tests run headlessly without a terminal, verifying that:
//! - Screen navigation works correctly
//! - Commands dispatch through the effect bridge
//! - Events are emitted and handled properly
//! - Recovery flow progresses through all stages
//! - Data flows correctly from views to screens

use std::sync::Arc;
use std::time::Duration;

use aura_core::effects::time::PhysicalTimeEffects;
use tokio::sync::RwLock;

// Import TUI types from aura-cli's public API
use aura_cli::demo::simulator_integration::SimulatedBridge;
use aura_cli::tui::{AuraEvent, DemoScenario, EffectCommand, EventFilter, RecoveryState};

/// Mock time effects for deterministic testing
#[derive(Debug, Clone)]
pub struct MockTimeEffects {
    current_time: Arc<RwLock<u64>>,
    sleep_calls: Arc<RwLock<Vec<u64>>>,
}

impl MockTimeEffects {
    pub fn new() -> Self {
        Self {
            current_time: Arc::new(RwLock::new(1700000000000)), // Fixed epoch
            sleep_calls: Arc::new(RwLock::new(Vec::new())),
        }
    }

    pub async fn advance(&self, ms: u64) {
        let mut time = self.current_time.write().await;
        *time += ms;
    }

    #[allow(dead_code)]
    pub async fn get_sleep_calls(&self) -> Vec<u64> {
        self.sleep_calls.read().await.clone()
    }
}

impl Default for MockTimeEffects {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl PhysicalTimeEffects for MockTimeEffects {
    async fn physical_time(&self) -> Result<aura_core::time::PhysicalTime, aura_core::AuraError> {
        let ts = *self.current_time.read().await;
        Ok(aura_core::time::PhysicalTime {
            ts_ms: ts,
            uncertainty_ms: None,
        })
    }

    async fn sleep_ms(&self, ms: u64) -> Result<(), aura_core::AuraError> {
        // Record the sleep call but don't actually sleep
        self.sleep_calls.write().await.push(ms);
        // Advance time instead
        self.advance(ms).await;
        Ok(())
    }
}

/// Test harness for headless TUI testing
///
/// Provides programmatic control over the TUI without needing a terminal.
/// Uses SimulatedBridge for mock backend behavior.
pub struct TuiTestHarness {
    /// The simulated bridge
    bridge: SimulatedBridge,
    /// Event subscription for verifying events
    event_sub: aura_cli::tui::effects::EventSubscription,
    /// Mock time effects for deterministic timing
    time_effects: Arc<MockTimeEffects>,
}

impl TuiTestHarness {
    /// Create a new test harness with happy path scenario
    pub fn new() -> Self {
        Self::with_scenario(DemoScenario::HappyPath)
    }

    /// Create a new test harness with a specific scenario
    pub fn with_scenario(scenario: DemoScenario) -> Self {
        let time_effects = Arc::new(MockTimeEffects::new());
        let bridge = SimulatedBridge::with_scenario_and_time(scenario, time_effects.clone());
        let event_sub = bridge.subscribe(EventFilter::all());

        Self {
            bridge,
            event_sub,
            time_effects,
        }
    }

    /// Initialize the demo with sample data
    pub async fn initialize(&self) {
        self.bridge.initialize().await;
    }

    /// Get a reference to the bridge
    pub fn bridge(&self) -> &SimulatedBridge {
        &self.bridge
    }

    /// Dispatch a command
    pub async fn dispatch(&self, command: EffectCommand) -> Result<(), String> {
        self.bridge.dispatch(command).await
    }

    /// Wait for a specific event type with timeout
    #[allow(dead_code)]
    pub async fn wait_for_event(
        &mut self,
        predicate: impl Fn(&AuraEvent) -> bool,
        timeout_ms: u64,
    ) -> Option<AuraEvent> {
        let deadline = tokio::time::Instant::now() + Duration::from_millis(timeout_ms);

        loop {
            // Check for event
            if let Some(event) = self.event_sub.try_recv() {
                if predicate(&event) {
                    return Some(event);
                }
            }

            // Check timeout
            if tokio::time::Instant::now() >= deadline {
                return None;
            }

            // Small yield to allow async tasks to progress
            tokio::task::yield_now().await;
        }
    }

    /// Try to receive an event without waiting
    pub fn try_recv_event(&mut self) -> Option<AuraEvent> {
        self.event_sub.try_recv()
    }

    /// Drain all pending events
    pub fn drain_events(&mut self) -> Vec<AuraEvent> {
        let mut events = Vec::new();
        while let Some(event) = self.event_sub.try_recv() {
            events.push(event);
        }
        events
    }

    /// Advance simulated time
    pub async fn advance_time(&self, ms: u64) {
        self.time_effects.advance(ms).await;
    }

    /// Get current simulated time
    #[allow(dead_code)]
    pub async fn current_time(&self) -> u64 {
        self.time_effects
            .physical_time()
            .await
            .map(|t| t.ts_ms)
            .unwrap_or(0)
    }

    // ─── High-Level Test Actions ───────────────────────────────────────────

    /// Simulate sending a chat message
    pub async fn send_message(&self, channel: &str, content: &str) -> Result<(), String> {
        self.dispatch(EffectCommand::SendMessage {
            channel: channel.to_string(),
            content: content.to_string(),
        })
        .await
    }

    /// Simulate starting recovery
    pub async fn start_recovery(&self) -> Result<(), String> {
        self.dispatch(EffectCommand::StartRecovery).await
    }

    /// Simulate canceling recovery
    pub async fn cancel_recovery(&self) -> Result<(), String> {
        self.dispatch(EffectCommand::CancelRecovery).await
    }

    /// Simulate completing recovery
    #[allow(dead_code)]
    pub async fn complete_recovery(&self) -> Result<(), String> {
        self.dispatch(EffectCommand::CompleteRecovery).await
    }

    /// Simulate guardian approval
    pub async fn submit_guardian_approval(&self, guardian_id: &str) -> Result<(), String> {
        self.dispatch(EffectCommand::SubmitGuardianApproval {
            guardian_id: guardian_id.to_string(),
        })
        .await
    }

    /// Ping the system
    pub async fn ping(&self) -> Result<(), String> {
        self.dispatch(EffectCommand::Ping).await
    }

    // ─── Assertion Helpers ─────────────────────────────────────────────────

    /// Assert the bridge is connected
    pub async fn assert_connected(&self) {
        assert!(
            self.bridge.is_connected().await,
            "Expected bridge to be connected"
        );
    }

    /// Assert no errors
    pub async fn assert_no_error(&self) {
        assert!(
            self.bridge.last_error().await.is_none(),
            "Expected no error, got: {:?}",
            self.bridge.last_error().await
        );
    }

    /// Assert message count in a channel
    pub async fn assert_message_count(&self, channel: &str, expected: usize) {
        let messages = self.store().get_messages(channel).await;
        assert_eq!(
            messages.len(),
            expected,
            "Expected {} messages in channel '{}', got {}",
            expected,
            channel,
            messages.len()
        );
    }

    /// Assert recovery state
    pub async fn assert_recovery_state(&self, expected: RecoveryState) {
        let recovery = self.store().get_recovery().await;
        assert_eq!(
            recovery.state, expected,
            "Expected recovery state {:?}, got {:?}",
            expected, recovery.state
        );
    }

    /// Assert approval count
    pub async fn assert_approval_count(&self, expected: usize) {
        let recovery = self.store().get_recovery().await;
        assert_eq!(
            recovery.approvals_received, expected,
            "Expected {} approvals, got {}",
            expected, recovery.approvals_received
        );
    }
}

impl Default for TuiTestHarness {
    fn default() -> Self {
        Self::new()
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use aura_cli::tui::effects::{AuraEvent, EffectCommand};
    use aura_cli::tui::reactive::RecoveryState;

    /// Test basic harness initialization
    #[tokio::test]
    async fn test_harness_initialization() {
        let harness = TuiTestHarness::new();
        harness.initialize().await;

        harness.assert_connected().await;
        harness.assert_no_error().await;

        // Verify demo data is loaded
        let channels = harness.store().get_channels().await;
        assert!(!channels.is_empty(), "Expected channels to be loaded");

        let guardians = harness.store().get_guardians().await;
        assert!(!guardians.is_empty(), "Expected guardians to be loaded");
    }

    /// Test ping/pong
    #[tokio::test]
    async fn test_ping_pong() {
        let mut harness = TuiTestHarness::new();

        // Send ping
        harness.ping().await.expect("Ping should succeed");

        // Should receive pong event
        let event = harness.try_recv_event();
        assert!(
            matches!(event, Some(AuraEvent::Pong { .. })),
            "Expected Pong event, got {:?}",
            event
        );
    }

    /// Test sending messages
    #[tokio::test]
    async fn test_send_message() {
        let mut harness = TuiTestHarness::new();
        harness.initialize().await;

        // Get initial message count
        let initial_count = harness.store().get_messages("general").await.len();

        // Send a message
        harness
            .send_message("general", "Hello, world!")
            .await
            .expect("Should send message");

        // Verify message was added
        harness.assert_message_count("general", initial_count + 1).await;

        // Should receive MessageReceived event
        let event = harness.try_recv_event();
        assert!(
            matches!(event, Some(AuraEvent::MessageReceived { .. })),
            "Expected MessageReceived event, got {:?}",
            event
        );
    }

    /// Test recovery initiation
    #[tokio::test]
    async fn test_recovery_initiation() {
        let mut harness =
            TuiTestHarness::with_scenario(aura_cli::tui::demo::DemoScenario::Interactive);
        harness.initialize().await;

        // Start recovery
        harness
            .start_recovery()
            .await
            .expect("Should start recovery");

        // Verify state changed
        harness.assert_recovery_state(RecoveryState::Initiated).await;

        // Should receive RecoveryStarted event
        let event = harness.try_recv_event();
        assert!(
            matches!(event, Some(AuraEvent::RecoveryStarted { .. })),
            "Expected RecoveryStarted event, got {:?}",
            event
        );
    }

    /// Test recovery cancellation
    #[tokio::test]
    async fn test_recovery_cancellation() {
        let mut harness =
            TuiTestHarness::with_scenario(aura_cli::tui::demo::DemoScenario::Interactive);
        harness.initialize().await;

        // Start then cancel recovery
        harness.start_recovery().await.expect("Should start");
        harness.drain_events(); // Clear the started event

        harness.cancel_recovery().await.expect("Should cancel");

        // Verify state changed
        harness.assert_recovery_state(RecoveryState::None).await;

        // Should receive RecoveryCancelled event
        let event = harness.try_recv_event();
        assert!(
            matches!(event, Some(AuraEvent::RecoveryCancelled { .. })),
            "Expected RecoveryCancelled event, got {:?}",
            event
        );
    }

    /// Test guardian approval flow
    #[tokio::test]
    async fn test_guardian_approval_flow() {
        let mut harness =
            TuiTestHarness::with_scenario(aura_cli::tui::demo::DemoScenario::Interactive);
        harness.initialize().await;

        // Start recovery
        harness.start_recovery().await.expect("Should start");
        harness.drain_events();

        // Get guardian IDs
        let guardians = harness.store().get_guardians().await;
        assert!(guardians.len() >= 2, "Need at least 2 guardians");

        // Submit first approval
        harness
            .submit_guardian_approval(&guardians[0].authority_id)
            .await
            .expect("Should approve");

        harness.assert_approval_count(1).await;

        // Should receive GuardianApproved event
        let event = harness.try_recv_event();
        assert!(
            matches!(event, Some(AuraEvent::GuardianApproved { .. })),
            "Expected GuardianApproved event, got {:?}",
            event
        );

        // Submit second approval (should meet threshold)
        harness
            .submit_guardian_approval(&guardians[1].authority_id)
            .await
            .expect("Should approve");

        harness.assert_approval_count(2).await;
        harness
            .assert_recovery_state(RecoveryState::Completed)
            .await;

        // Should receive GuardianApproved and ThresholdMet events
        let events = harness.drain_events();
        let has_threshold_met = events
            .iter()
            .any(|e| matches!(e, AuraEvent::ThresholdMet { .. }));
        assert!(has_threshold_met, "Expected ThresholdMet event");
    }

    /// Test complete demo flow (happy path)
    #[tokio::test]
    async fn test_complete_demo_flow() {
        let mut harness = TuiTestHarness::new();
        harness.initialize().await;

        // Phase 1: Initial state
        harness.assert_connected().await;
        let channels = harness.store().get_channels().await;
        assert!(!channels.is_empty());

        // Phase 2: Send some messages
        harness.send_message("general", "Hello!").await.unwrap();
        harness
            .send_message("general", "Testing demo flow")
            .await
            .unwrap();
        harness.drain_events();

        // Phase 3: Start recovery
        harness.start_recovery().await.unwrap();

        // In happy path, guardians should auto-approve
        // Wait a bit for scheduled guardian responses
        harness.advance_time(1000).await;
        tokio::task::yield_now().await;

        // The mock time means scheduled tasks will see time has advanced
        // But we may need to wait for the async tasks to process
        tokio::time::sleep(Duration::from_millis(50)).await;

        // Check that recovery progressed (either approval or completion)
        let recovery = harness.store().get_recovery().await;
        assert!(
            recovery.approvals_received > 0 || recovery.state == RecoveryState::Completed,
            "Expected recovery to progress, got {:?} approvals, state {:?}",
            recovery.approvals_received,
            recovery.state
        );
    }

    /// Test slow guardian scenario
    #[tokio::test]
    async fn test_slow_guardian_scenario() {
        let harness =
            TuiTestHarness::with_scenario(aura_cli::tui::demo::DemoScenario::SlowGuardian);
        harness.initialize().await;

        // Verify scenario is correct
        assert_eq!(
            harness.bridge().scenario(),
            aura_cli::tui::demo::DemoScenario::SlowGuardian
        );

        // Start recovery
        harness.start_recovery().await.unwrap();

        // The delays should be different (500ms vs 5000ms)
        let (delay1, delay2) = harness.bridge().scenario().guardian_delays();
        assert_ne!(
            delay1, delay2,
            "Slow guardian scenario should have different delays"
        );
    }

    /// Test interactive scenario (no auto-advance)
    #[tokio::test]
    async fn test_interactive_scenario() {
        let mut harness =
            TuiTestHarness::with_scenario(aura_cli::tui::demo::DemoScenario::Interactive);
        harness.initialize().await;

        // Verify scenario doesn't auto-advance
        assert!(!harness.bridge().scenario().auto_advance());

        // Start recovery
        harness.start_recovery().await.unwrap();
        harness.drain_events();

        // Wait - in interactive mode, guardians should NOT auto-approve
        harness.advance_time(5000).await;
        tokio::task::yield_now().await;

        // Recovery should still be in initiated state
        harness.assert_recovery_state(RecoveryState::Initiated).await;
        harness.assert_approval_count(0).await;

        // Manually submit approvals
        let guardians = harness.store().get_guardians().await;
        harness
            .submit_guardian_approval(&guardians[0].authority_id)
            .await
            .unwrap();
        harness.assert_approval_count(1).await;
    }

    /// Test error state handling
    #[tokio::test]
    async fn test_error_state() {
        let harness = TuiTestHarness::new();

        // Initially no error
        harness.assert_no_error().await;

        // Set an error
        harness.bridge().set_error("Test error").await;

        // Should have error
        let error = harness.bridge().last_error().await;
        assert_eq!(error, Some("Test error".to_string()));

        // Clear error
        harness.bridge().clear_error().await;
        harness.assert_no_error().await;
    }

    /// Test connection state
    #[tokio::test]
    async fn test_connection_state() {
        let harness = TuiTestHarness::new();

        // Initially connected
        harness.assert_connected().await;

        // Disconnect
        harness.bridge().set_connected(false).await;
        assert!(!harness.bridge().is_connected().await);

        // Reconnect
        harness.bridge().set_connected(true).await;
        harness.assert_connected().await;
    }

    /// Test multiple message channels
    #[tokio::test]
    async fn test_multiple_channels() {
        let harness = TuiTestHarness::new();
        harness.initialize().await;

        // Get channels
        let channels = harness.store().get_channels().await;
        assert!(channels.len() >= 2, "Expected multiple channels");

        // Send messages to different channels
        harness.send_message("general", "General message").await.unwrap();
        harness.send_message("guardians", "Guardian message").await.unwrap();

        // Verify messages went to correct channels
        let general_msgs = harness.store().get_messages("general").await;
        let guardian_msgs = harness.store().get_messages("guardians").await;

        assert!(
            general_msgs.iter().any(|m| m.content == "General message"),
            "Message should be in general channel"
        );
        assert!(
            guardian_msgs.iter().any(|m| m.content == "Guardian message"),
            "Message should be in guardians channel"
        );
    }

    /// Test data consistency through demo flow
    #[tokio::test]
    async fn test_data_consistency() {
        let mut harness =
            TuiTestHarness::with_scenario(aura_cli::tui::demo::DemoScenario::Interactive);
        harness.initialize().await;

        // Record initial state
        let initial_guardians = harness.store().get_guardians().await;
        let guardian_count = initial_guardians.len();

        // Start recovery
        harness.start_recovery().await.unwrap();

        // Guardian count should remain unchanged
        let current_guardians = harness.store().get_guardians().await;
        assert_eq!(
            current_guardians.len(),
            guardian_count,
            "Guardian count should not change during recovery"
        );

        // Submit all approvals
        for guardian in &initial_guardians {
            harness
                .submit_guardian_approval(&guardian.authority_id)
                .await
                .unwrap();
        }

        // All guardians should still exist
        let final_guardians = harness.store().get_guardians().await;
        assert_eq!(
            final_guardians.len(),
            guardian_count,
            "Guardian count should remain stable"
        );
    }
}
