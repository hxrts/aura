//! # Human-Agent Demo Mode
//!
//! Provides Bob's real user experience with automated Alice/Charlie guardians.
//! This mode integrates the TUI interface with simulator agents to create
//! a complete demo where Bob has the full interactive experience while
//! Alice and Charlie are automated for reliable demo presentation.

use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};
use uuid::Uuid;

use super::{
    scenario_bridge::{DemoScenarioBridge, DemoSetupConfig},
    simulator_integration::SimulatedGuardianAgent,
};
use crate::tui::{DemoEvent, TuiApp};
use aura_core::AuthorityId;

/// Human-agent demo coordinator
///
/// Orchestrates a demo where:
/// - Bob is a real user with full TUI interface
/// - Alice and Charlie are automated agents
/// - Demo progression is reliable and predictable
/// - Real Aura protocols are exercised end-to-end
pub struct HumanAgentDemo {
    /// Bob's TUI application
    bob_app: TuiApp,

    /// Alice's automated agent
    alice_agent: Arc<Mutex<SimulatedGuardianAgent>>,

    /// Charlie's automated agent  
    charlie_agent: Arc<Mutex<SimulatedGuardianAgent>>,

    /// Demo event channel for coordination
    demo_events: mpsc::UnboundedReceiver<DemoEvent>,

    /// Demo state
    demo_state: DemoState,

    /// Demo configuration
    config: HumanAgentDemoConfig,
}

/// Demo state tracking
#[derive(Debug, Clone)]
pub struct DemoState {
    /// Bob's authority ID
    pub bob_authority: Option<AuthorityId>,

    /// Alice's authority ID
    pub alice_authority: Option<AuthorityId>,

    /// Charlie's authority ID  
    pub charlie_authority: Option<AuthorityId>,

    /// Current demo phase
    pub phase: DemoPhase,

    /// Recovery session ID if active
    pub recovery_session: Option<Uuid>,

    /// Demo metrics
    pub metrics: DemoMetrics,
}

/// Demo phase progression
#[derive(Debug, Clone, PartialEq)]
pub enum DemoPhase {
    /// Initial setup
    Setup,

    /// Bob onboarding
    BobOnboarding,

    /// Guardian setup (Alice & Charlie)
    GuardianSetup,

    /// Normal operation (group chat)
    NormalOperation,

    /// Simulate device loss
    DeviceLoss,

    /// Recovery initiation
    RecoveryInitiation,

    /// Guardian coordination
    GuardianCoordination,

    /// Recovery completion
    RecoveryCompletion,

    /// Demo completed
    Completed,
}

/// Demo metrics for monitoring
#[derive(Debug, Clone, Default)]
pub struct DemoMetrics {
    /// Messages exchanged
    pub messages_sent: usize,

    /// Recovery operations performed
    pub recovery_operations: usize,

    /// Guardian approvals received
    pub guardian_approvals: usize,

    /// Demo start time
    pub start_time: Option<std::time::Instant>,

    /// Demo duration
    pub duration: Option<std::time::Duration>,
}

/// Configuration for human-agent demo
#[derive(Debug, Clone)]
pub struct HumanAgentDemoConfig {
    /// Enable automatic phase advancement
    pub auto_advance: bool,

    /// Delay between automated actions (ms)
    pub agent_delay_ms: u64,

    /// Enable detailed logging
    pub verbose_logging: bool,

    /// Guardian response time simulation (ms)
    pub guardian_response_time_ms: u64,

    /// Maximum demo duration before auto-completion
    pub max_demo_duration_minutes: u64,
}

impl Default for HumanAgentDemoConfig {
    fn default() -> Self {
        Self {
            auto_advance: true,
            agent_delay_ms: 1000,
            verbose_logging: true,
            guardian_response_time_ms: 3000,
            max_demo_duration_minutes: 30,
        }
    }
}

impl HumanAgentDemo {
    /// Create new human-agent demo
    pub async fn new(config: HumanAgentDemoConfig) -> anyhow::Result<Self> {
        let (demo_tx, demo_rx) = mpsc::unbounded_channel();

        let mut bob_app = TuiApp::new();
        bob_app.set_demo_sender(demo_tx);

        // Use scenario bridge to setup the complete demo environment
        let seed = 42; // Deterministic seed for reliable demo
        let setup_config = DemoSetupConfig {
            participant_count: 3,
            guardian_threshold: 2,
            setup_chat_history: true,
            initial_message_count: 5,
            verbose_logging: config.verbose_logging,
            simulate_network_activity: true,
        };

        let bridge = DemoScenarioBridge::new(seed, setup_config);
        let setup_result = bridge.setup_demo_environment().await?;

        let alice_agent = Arc::new(Mutex::new(setup_result.agents.0));
        let charlie_agent = Arc::new(Mutex::new(setup_result.agents.1));

        tracing::info!(
            "Human-agent demo initialized with scenario bridge (setup took {:.2}s)",
            setup_result.setup_metrics.setup_duration.as_secs_f64()
        );

        Ok(Self {
            bob_app,
            alice_agent,
            charlie_agent,
            demo_events: demo_rx,
            demo_state: setup_result.demo_state,
            config,
        })
    }

    /// Run the human-agent demo
    pub async fn run(&mut self) -> anyhow::Result<()> {
        tracing::info!("Starting human-agent demo");

        // Initialize demo metrics
        self.demo_state.metrics.start_time = Some(std::time::Instant::now());

        // Start Bob's TUI in background
        let bob_handle = {
            let mut app = std::mem::take(&mut self.bob_app);
            tokio::spawn(async move {
                if let Err(e) = app.run().await {
                    tracing::error!("Bob TUI error: {}", e);
                }
            })
        };

        // Start automated agents
        self.start_automated_agents().await?;

        // Main demo loop
        self.run_demo_loop().await?;

        // Wait for Bob's TUI to complete
        let _ = bob_handle.await;

        // Finalize metrics
        if let Some(start_time) = self.demo_state.metrics.start_time {
            self.demo_state.metrics.duration = Some(start_time.elapsed());
        }

        tracing::info!("Human-agent demo completed");
        self.print_demo_summary();

        Ok(())
    }

    /// Start automated agent tasks
    async fn start_automated_agents(&mut self) -> anyhow::Result<()> {
        tracing::info!("Starting automated guardian agents");

        // Start Alice agent
        let alice_agent = Arc::clone(&self.alice_agent);
        let alice_config = self.config.clone();
        tokio::spawn(async move {
            Self::run_alice_agent(alice_agent, alice_config).await;
        });

        // Start Charlie agent
        let charlie_agent = Arc::clone(&self.charlie_agent);
        let charlie_config = self.config.clone();
        tokio::spawn(async move {
            Self::run_charlie_agent(charlie_agent, charlie_config).await;
        });

        Ok(())
    }

    /// Main demo coordination loop
    async fn run_demo_loop(&mut self) -> anyhow::Result<()> {
        let max_duration =
            std::time::Duration::from_secs(self.config.max_demo_duration_minutes * 60);

        let start_time = std::time::Instant::now();

        while start_time.elapsed() < max_duration {
            // Handle demo events from Bob's TUI
            if let Ok(event) = tokio::time::timeout(
                std::time::Duration::from_millis(100),
                self.demo_events.recv(),
            )
            .await
            {
                if let Some(event) = event {
                    self.handle_demo_event(event).await?;

                    if self.demo_state.phase == DemoPhase::Completed {
                        break;
                    }
                }
            }

            // Check for automatic phase advancement
            if self.config.auto_advance {
                self.check_auto_advance().await?;
            }
        }

        Ok(())
    }

    /// Handle events from Bob's TUI
    async fn handle_demo_event(&mut self, event: DemoEvent) -> anyhow::Result<()> {
        match event {
            DemoEvent::AdvancePhase => {
                self.advance_demo_phase().await?;
            }
            DemoEvent::SendMessage(content) => {
                self.handle_message(content).await?;
            }
            DemoEvent::InitiateRecovery => {
                self.initiate_recovery_process().await?;
            }
            DemoEvent::GuardianApproval(guardian_id) => {
                self.handle_guardian_approval(guardian_id).await?;
            }
            DemoEvent::RestoreMessages => {
                self.complete_recovery().await?;
            }
            DemoEvent::Reset => {
                self.reset_demo().await?;
            }
        }

        Ok(())
    }

    /// Advance to next demo phase
    async fn advance_demo_phase(&mut self) -> anyhow::Result<()> {
        let next_phase = match self.demo_state.phase {
            DemoPhase::Setup => DemoPhase::BobOnboarding,
            DemoPhase::BobOnboarding => DemoPhase::GuardianSetup,
            DemoPhase::GuardianSetup => DemoPhase::NormalOperation,
            DemoPhase::NormalOperation => DemoPhase::DeviceLoss,
            DemoPhase::DeviceLoss => DemoPhase::RecoveryInitiation,
            DemoPhase::RecoveryInitiation => DemoPhase::GuardianCoordination,
            DemoPhase::GuardianCoordination => DemoPhase::RecoveryCompletion,
            DemoPhase::RecoveryCompletion => DemoPhase::Completed,
            DemoPhase::Completed => return Ok(()), // Already completed
        };

        self.demo_state.phase = next_phase.clone();
        tracing::info!("Demo phase advanced to: {:?}", next_phase);

        // Trigger phase-specific automation
        match next_phase {
            DemoPhase::GuardianSetup => {
                self.setup_guardians().await?;
            }
            DemoPhase::NormalOperation => {
                self.start_normal_operation().await?;
            }
            DemoPhase::DeviceLoss => {
                self.simulate_device_loss().await?;
            }
            DemoPhase::GuardianCoordination => {
                self.trigger_guardian_responses().await?;
            }
            _ => {}
        }

        Ok(())
    }

    /// Setup automated guardians
    async fn setup_guardians(&mut self) -> anyhow::Result<()> {
        tracing::info!("Setting up automated guardians");

        // Simulate guardian registration with delays
        tokio::time::sleep(std::time::Duration::from_millis(self.config.agent_delay_ms)).await;

        // Alice setup
        {
            let mut alice = self.alice_agent.lock().await;
            alice.register_as_guardian().await?;
        }

        tokio::time::sleep(std::time::Duration::from_millis(
            self.config.agent_delay_ms / 2,
        ))
        .await;

        // Charlie setup
        {
            let mut charlie = self.charlie_agent.lock().await;
            charlie.register_as_guardian().await?;
        }

        tracing::info!("Guardian setup completed");
        Ok(())
    }

    /// Start normal operation phase
    async fn start_normal_operation(&mut self) -> anyhow::Result<()> {
        tracing::info!("Starting normal operation phase");

        // Simulate some automated guardian activity
        self.simulate_guardian_activity().await?;

        Ok(())
    }

    /// Simulate device loss
    async fn simulate_device_loss(&mut self) -> anyhow::Result<()> {
        tracing::info!("Simulating Bob's device loss");

        // This would integrate with the actual Aura system
        // For now, we'll just log and update state
        self.demo_state.bob_authority = None;

        Ok(())
    }

    /// Initiate recovery process
    async fn initiate_recovery_process(&mut self) -> anyhow::Result<()> {
        tracing::info!("Initiating recovery process");

        let recovery_session = Uuid::new_v4();
        self.demo_state.recovery_session = Some(recovery_session);
        self.demo_state.metrics.recovery_operations += 1;

        // Advance to guardian coordination phase
        self.demo_state.phase = DemoPhase::GuardianCoordination;

        Ok(())
    }

    /// Trigger automated guardian responses
    async fn trigger_guardian_responses(&mut self) -> anyhow::Result<()> {
        tracing::info!("Triggering automated guardian responses");

        let response_delay =
            std::time::Duration::from_millis(self.config.guardian_response_time_ms);

        // Alice approves after delay
        let _alice_authority = self.demo_state.alice_authority.unwrap();
        tokio::spawn(async move {
            tokio::time::sleep(response_delay).await;
            // Would send approval through actual system
            tracing::info!("Alice automatically approved recovery");
        });

        // Charlie approves after different delay
        let _charlie_authority = self.demo_state.charlie_authority.unwrap();
        let charlie_delay = response_delay + std::time::Duration::from_millis(1000);
        tokio::spawn(async move {
            tokio::time::sleep(charlie_delay).await;
            // Would send approval through actual system
            tracing::info!("Charlie automatically approved recovery");
        });

        Ok(())
    }

    /// Handle message sending
    async fn handle_message(&mut self, content: String) -> anyhow::Result<()> {
        tracing::info!("Bob sent message: {}", content);
        self.demo_state.metrics.messages_sent += 1;

        // Simulate automated guardian responses to messages
        self.simulate_guardian_message_responses(&content).await?;

        Ok(())
    }

    /// Handle guardian approval
    async fn handle_guardian_approval(&mut self, guardian_id: AuthorityId) -> anyhow::Result<()> {
        tracing::info!("Guardian approval received from: {}", guardian_id);
        self.demo_state.metrics.guardian_approvals += 1;

        // Check if threshold reached (2-of-3)
        if self.demo_state.metrics.guardian_approvals >= 2 {
            self.advance_demo_phase().await?;
        }

        Ok(())
    }

    /// Complete recovery process
    async fn complete_recovery(&mut self) -> anyhow::Result<()> {
        tracing::info!("Completing recovery process");

        // Restore Bob's authority
        self.demo_state.bob_authority = Some(AuthorityId::new());
        self.demo_state.recovery_session = None;

        // Advance to completion
        self.demo_state.phase = DemoPhase::Completed;

        Ok(())
    }

    /// Reset demo state
    async fn reset_demo(&mut self) -> anyhow::Result<()> {
        tracing::info!("Resetting demo");

        self.demo_state = DemoState {
            bob_authority: None,
            alice_authority: self.demo_state.alice_authority,
            charlie_authority: self.demo_state.charlie_authority,
            phase: DemoPhase::Setup,
            recovery_session: None,
            metrics: DemoMetrics::default(),
        };

        Ok(())
    }

    /// Check for automatic phase advancement
    async fn check_auto_advance(&mut self) -> anyhow::Result<()> {
        // Implementation would check various conditions for auto-advancement
        // For now, just a placeholder
        Ok(())
    }

    /// Simulate guardian activity
    async fn simulate_guardian_activity(&self) -> anyhow::Result<()> {
        // Simulate periodic guardian heartbeats, status updates, etc.
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(10)).await;
                tracing::debug!("Guardian heartbeat");
            }
        });

        Ok(())
    }

    /// Simulate guardian message responses
    async fn simulate_guardian_message_responses(&self, _content: &str) -> anyhow::Result<()> {
        // Would analyze Bob's message and generate appropriate guardian responses
        // For demo purposes, just simulate some activity

        tokio::time::sleep(std::time::Duration::from_millis(
            self.config.agent_delay_ms / 2,
        ))
        .await;

        tracing::debug!("Guardians acknowledged Bob's message");
        Ok(())
    }

    /// Print demo summary
    fn print_demo_summary(&self) {
        println!("\n=== Demo Summary ===");
        println!("Phase: {:?}", self.demo_state.phase);
        println!("Messages sent: {}", self.demo_state.metrics.messages_sent);
        println!(
            "Recovery operations: {}",
            self.demo_state.metrics.recovery_operations
        );
        println!(
            "Guardian approvals: {}",
            self.demo_state.metrics.guardian_approvals
        );

        if let Some(duration) = self.demo_state.metrics.duration {
            println!("Duration: {:.2}s", duration.as_secs_f64());
        }

        println!("====================\n");
    }

    /// Run Alice's automated agent
    async fn run_alice_agent(
        agent: Arc<Mutex<SimulatedGuardianAgent>>,
        config: HumanAgentDemoConfig,
    ) {
        loop {
            tokio::time::sleep(std::time::Duration::from_millis(config.agent_delay_ms)).await;

            // Alice's automated behavior
            {
                let mut alice = agent.lock().await;
                if let Err(e) = alice.process_pending_actions().await {
                    tracing::error!("Alice agent error: {}", e);
                }
            }
        }
    }

    /// Run Charlie's automated agent
    async fn run_charlie_agent(
        agent: Arc<Mutex<SimulatedGuardianAgent>>,
        config: HumanAgentDemoConfig,
    ) {
        loop {
            tokio::time::sleep(std::time::Duration::from_millis(
                config.agent_delay_ms + 500, // Different timing from Alice
            ))
            .await;

            // Charlie's automated behavior
            {
                let mut charlie = agent.lock().await;
                if let Err(e) = charlie.process_pending_actions().await {
                    tracing::error!("Charlie agent error: {}", e);
                }
            }
        }
    }
}

impl Default for DemoState {
    fn default() -> Self {
        Self {
            bob_authority: None,
            alice_authority: None,
            charlie_authority: None,
            phase: DemoPhase::Setup,
            recovery_session: None,
            metrics: DemoMetrics::default(),
        }
    }
}

/// Demo builder for easy configuration
pub struct HumanAgentDemoBuilder {
    config: HumanAgentDemoConfig,
}

impl HumanAgentDemoBuilder {
    /// Create new demo builder
    pub fn new() -> Self {
        Self {
            config: HumanAgentDemoConfig::default(),
        }
    }

    /// Set auto-advance enabled
    pub fn with_auto_advance(mut self, enabled: bool) -> Self {
        self.config.auto_advance = enabled;
        self
    }

    /// Set agent delay
    pub fn with_agent_delay_ms(mut self, delay_ms: u64) -> Self {
        self.config.agent_delay_ms = delay_ms;
        self
    }

    /// Set verbose logging
    pub fn with_verbose_logging(mut self, enabled: bool) -> Self {
        self.config.verbose_logging = enabled;
        self
    }

    /// Set guardian response time
    pub fn with_guardian_response_time_ms(mut self, time_ms: u64) -> Self {
        self.config.guardian_response_time_ms = time_ms;
        self
    }

    /// Set max demo duration
    pub fn with_max_duration_minutes(mut self, minutes: u64) -> Self {
        self.config.max_demo_duration_minutes = minutes;
        self
    }

    /// Build the demo
    pub async fn build(self) -> anyhow::Result<HumanAgentDemo> {
        HumanAgentDemo::new(self.config).await
    }
}

impl Default for HumanAgentDemoBuilder {
    fn default() -> Self {
        Self::new()
    }
}
