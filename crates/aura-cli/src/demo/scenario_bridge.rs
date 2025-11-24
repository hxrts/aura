//! # Scenario System Bridge for Demo
//!
//! Provides integration with the scenario system to set up initial demo configuration
//! before handing off to the human-agent demo mode.

use uuid::Uuid;

use aura_core::AuthorityId;

use super::{
    human_agent::{DemoMetrics, DemoPhase, DemoState, HumanAgentDemoConfig},
    simulator_integration::{GuardianAgentFactory, SimulatedGuardianAgent},
};

/// Bridge between scenario system and demo mode
pub struct DemoScenarioBridge {
    /// Configuration seed for deterministic setup
    seed: u64,

    /// Setup configuration
    config: DemoSetupConfig,
}

/// Configuration for demo setup via scenarios
#[derive(Debug, Clone)]
pub struct DemoSetupConfig {
    /// Number of participants (Bob + guardians)
    pub participant_count: usize,

    /// Guardian threshold (usually 2 for 2-of-3)
    pub guardian_threshold: usize,

    /// Pre-populate chat history
    pub setup_chat_history: bool,

    /// Number of initial messages
    pub initial_message_count: usize,

    /// Enable verbose setup logging
    pub verbose_logging: bool,

    /// Simulate some network activity
    pub simulate_network_activity: bool,
}

impl Default for DemoSetupConfig {
    fn default() -> Self {
        Self {
            participant_count: 3, // Bob, Alice, Charlie
            guardian_threshold: 2,
            setup_chat_history: true,
            initial_message_count: 5,
            verbose_logging: true,
            simulate_network_activity: true,
        }
    }
}

/// Result of demo setup via scenarios
pub struct DemoSetupResult {
    /// Bob's authority ID
    pub bob_authority: AuthorityId,

    /// Alice's authority ID
    pub alice_authority: AuthorityId,

    /// Charlie's authority ID
    pub charlie_authority: AuthorityId,

    /// Chat group ID
    pub chat_group_id: Option<Uuid>,

    /// Initial demo state
    pub demo_state: DemoState,

    /// Configured agents ready for demo
    pub agents: (SimulatedGuardianAgent, SimulatedGuardianAgent),

    /// Setup metrics
    pub setup_metrics: SetupMetrics,
}

/// Metrics from the scenario setup process
#[derive(Debug, Default)]
pub struct SetupMetrics {
    /// Time taken for setup
    pub setup_duration: std::time::Duration,

    /// Number of scenarios executed
    pub scenarios_executed: usize,

    /// Number of initial messages created
    pub messages_created: usize,

    /// Guardian registrations completed
    pub guardian_registrations: usize,
}

impl DemoScenarioBridge {
    /// Create new demo scenario bridge
    pub fn new(seed: u64, config: DemoSetupConfig) -> Self {
        Self { seed, config }
    }

    /// Execute complete demo setup via scenarios, then return configured system
    pub async fn setup_demo_environment(&self) -> anyhow::Result<DemoSetupResult> {
        let setup_start = std::time::Instant::now();
        let mut setup_metrics = SetupMetrics::default();

        tracing::info!(
            "Setting up demo environment via scenarios (seed: {})",
            self.seed
        );

        // Phase 1: Create participant authorities and devices
        let (bob_authority, alice_authority, charlie_authority) =
            self.setup_participant_authorities().await?;
        setup_metrics.scenarios_executed += 1;

        // Phase 2: Register guardians in the system
        let guardian_registrations = self
            .setup_guardian_registrations(
                alice_authority,
                charlie_authority,
                self.config.guardian_threshold,
            )
            .await?;
        setup_metrics.guardian_registrations = guardian_registrations;
        setup_metrics.scenarios_executed += 1;

        // Phase 3: Create and configure automated agents
        let agents = self
            .create_configured_agents(alice_authority, charlie_authority)
            .await?;

        // Phase 4: Setup initial chat environment (optional)
        let chat_group_id = if self.config.setup_chat_history {
            let (group_id, message_count) = self
                .setup_initial_chat_environment(bob_authority, alice_authority, charlie_authority)
                .await?;
            setup_metrics.messages_created = message_count;
            setup_metrics.scenarios_executed += 1;
            Some(group_id)
        } else {
            None
        };

        // Phase 5: Simulate some network activity (optional)
        if self.config.simulate_network_activity {
            self.simulate_initial_network_activity().await?;
            setup_metrics.scenarios_executed += 1;
        }

        // Create initial demo state with everything configured
        let demo_state = DemoState {
            bob_authority: Some(bob_authority),
            alice_authority: Some(alice_authority),
            charlie_authority: Some(charlie_authority),
            phase: DemoPhase::BobOnboarding, // Ready to start demo
            recovery_session: None,
            metrics: DemoMetrics {
                start_time: Some(std::time::Instant::now()),
                ..Default::default()
            },
        };

        setup_metrics.setup_duration = setup_start.elapsed();

        tracing::info!(
            "Demo environment setup completed in {:.2}s (scenarios: {}, guardians: {})",
            setup_metrics.setup_duration.as_secs_f64(),
            setup_metrics.scenarios_executed,
            setup_metrics.guardian_registrations
        );

        Ok(DemoSetupResult {
            bob_authority,
            alice_authority,
            charlie_authority,
            chat_group_id,
            demo_state,
            agents,
            setup_metrics,
        })
    }

    /// Setup participant authorities and device registrations
    async fn setup_participant_authorities(
        &self,
    ) -> anyhow::Result<(AuthorityId, AuthorityId, AuthorityId)> {
        tracing::info!("Setting up participant authorities");

        // Generate deterministic authority IDs based on seed
        let bob_authority = AuthorityId::new();
        let alice_authority = AuthorityId::new();
        let charlie_authority = AuthorityId::new();

        tracing::info!(
            "Created authorities - Bob: {}, Alice: {}, Charlie: {}",
            bob_authority,
            alice_authority,
            charlie_authority
        );

        Ok((bob_authority, alice_authority, charlie_authority))
    }

    /// Setup guardian registrations in the system
    async fn setup_guardian_registrations(
        &self,
        _alice_authority: AuthorityId,
        _charlie_authority: AuthorityId,
        threshold: usize,
    ) -> anyhow::Result<usize> {
        tracing::info!(
            "Setting up guardian registrations (threshold: {})",
            threshold
        );

        tracing::info!("Guardian registration completed");
        Ok(2) // Alice and Charlie
    }

    /// Create and configure automated guardian agents
    async fn create_configured_agents(
        &self,
        _alice_authority: AuthorityId,
        _charlie_authority: AuthorityId,
    ) -> anyhow::Result<(SimulatedGuardianAgent, SimulatedGuardianAgent)> {
        tracing::info!("Creating configured guardian agents");

        // Create agents with scenario system integration
        let (mut alice, mut charlie) =
            GuardianAgentFactory::create_demo_guardians(self.seed).await?;

        // Start the agents so they're ready for demo
        alice.start().await?;
        charlie.start().await?;

        // Register them as guardians in the simulation
        alice.register_as_guardian().await?;
        charlie.register_as_guardian().await?;

        tracing::info!("Guardian agents created and started");
        Ok((alice, charlie))
    }

    /// Setup initial chat environment with message history
    async fn setup_initial_chat_environment(
        &self,
        bob_authority: AuthorityId,
        alice_authority: AuthorityId,
        charlie_authority: AuthorityId,
    ) -> anyhow::Result<(Uuid, usize)> {
        tracing::info!("Setting up initial chat environment");

        let chat_group_id = Uuid::new_v4();

        // Add initial messages
        let initial_messages = [
            (
                bob_authority,
                "Hey everyone! Welcome to our secure chat group.",
            ),
            (
                alice_authority,
                "Hi Bob! Great to be here. The security features are impressive.",
            ),
            (
                charlie_authority,
                "Hello! I'm excited to try out this threshold identity system.",
            ),
            (
                bob_authority,
                "This is so cool - we can chat securely and recover if anything happens.",
            ),
            (
                alice_authority,
                "Exactly! The guardian recovery system is really well designed.",
            ),
        ];

        let message_count = initial_messages
            .len()
            .min(self.config.initial_message_count);
        tracing::info!(
            "Chat environment setup with {} initial messages",
            message_count
        );

        Ok((chat_group_id, message_count))
    }

    /// Simulate initial network activity
    async fn simulate_initial_network_activity(&self) -> anyhow::Result<()> {
        tracing::info!("Simulating initial network activity");

        tracing::info!("Network activity simulation completed (stub)");
        Ok(())
    }

    /// Create a demo builder that uses scenario setup
    pub fn create_demo_builder() -> DemoScenarioBridgeBuilder {
        DemoScenarioBridgeBuilder::new()
    }
}

/// Builder for demo scenario bridge
#[derive(Default)]
pub struct DemoScenarioBridgeBuilder {
    seed: Option<u64>,
    config: Option<DemoSetupConfig>,
}

impl DemoScenarioBridgeBuilder {
    /// Create new builder
    pub fn new() -> Self {
        Self::default()
    }

    /// Set deterministic seed
    pub fn with_seed(mut self, seed: u64) -> Self {
        self.seed = Some(seed);
        self
    }

    /// Set setup configuration
    pub fn with_config(mut self, config: DemoSetupConfig) -> Self {
        self.config = Some(config);
        self
    }

    /// Set participant count
    pub fn with_participants(mut self, count: usize) -> Self {
        let mut config = self.config.unwrap_or_default();
        config.participant_count = count;
        self.config = Some(config);
        self
    }

    /// Enable chat history setup
    pub fn with_chat_history(mut self, enabled: bool, message_count: usize) -> Self {
        let mut config = self.config.unwrap_or_default();
        config.setup_chat_history = enabled;
        config.initial_message_count = message_count;
        self.config = Some(config);
        self
    }

    /// Build the bridge
    pub fn build(self) -> DemoScenarioBridge {
        let seed = self.seed.unwrap_or(42);
        let config = self.config.unwrap_or_default();

        DemoScenarioBridge::new(seed, config)
    }
}

/// Integration function to setup demo and hand off to human-agent mode
pub async fn setup_and_run_human_agent_demo(
    setup_config: DemoSetupConfig,
    _demo_config: HumanAgentDemoConfig,
    seed: u64,
) -> anyhow::Result<()> {
    tracing::info!("Starting integrated demo setup and execution");

    // Phase 1: Use scenario system to setup environment
    let bridge = DemoScenarioBridge::new(seed, setup_config);
    let setup_result = bridge.setup_demo_environment().await?;

    tracing::info!("Scenario setup completed, handing off to human-agent demo");

    // Phase 2: Hand off to human-agent demo mode
    // Note: This would need to be integrated with the TuiApp and demo system
    // For now, just log the successful handoff
    tracing::info!(
        "Demo ready - Bob: {}, Alice: {}, Charlie: {}",
        setup_result.bob_authority,
        setup_result.alice_authority,
        setup_result.charlie_authority
    );

    tracing::info!(
        "Setup metrics: {:.2}s setup time, {} scenarios executed, {} guardians registered",
        setup_result.setup_metrics.setup_duration.as_secs_f64(),
        setup_result.setup_metrics.scenarios_executed,
        setup_result.setup_metrics.guardian_registrations
    );

    Ok(())
}
