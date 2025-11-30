#![allow(deprecated)]
//! # Simulator Integration for Demo
//!
//! Integrates aura-simulator with the demo system to provide automated
//! Alice and Charlie agents for Bob's recovery demo experience.

use aura_core::PhysicalTimeEffects;
use aura_effects::time::PhysicalTimeHandler;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::Mutex;

use aura_core::{AuthorityId, DeviceId};
use aura_simulator::{ComposedSimulationEnvironment, SimulationEffectComposer};

use super::human_agent::{DemoPhase, DemoState};
use crate::tui::DemoEvent;

/// Simulator-backed automated agent for demo
pub struct SimulatedGuardianAgent {
    /// Guardian name (Alice or Charlie)
    name: String,

    /// Authority ID
    authority_id: AuthorityId,

    /// Device ID for simulation
    device_id: DeviceId,

    /// Simulation environment
    environment: Arc<Mutex<ComposedSimulationEnvironment>>,

    /// Agent configuration
    config: GuardianAgentConfig,

    /// Agent state
    state: GuardianAgentState,
}

/// Configuration for guardian agent behavior
#[derive(Debug, Clone)]
pub struct GuardianAgentConfig {
    /// Simulation seed for deterministic behavior
    pub seed: u64,

    /// Response delay range (min, max) in milliseconds
    pub response_delay_ms: (u64, u64),

    /// Guardian approval probability (0.0-1.0)
    pub approval_probability: f64,

    /// Enable fault injection
    pub enable_faults: bool,

    /// Message generation frequency
    pub message_frequency_ms: u64,

    /// Enable verbose simulation logging
    pub verbose_logging: bool,
}

impl Default for GuardianAgentConfig {
    fn default() -> Self {
        Self {
            seed: 42,
            response_delay_ms: (1000, 5000),
            approval_probability: 0.95, // Very reliable for demo
            enable_faults: false,
            message_frequency_ms: 10000,
            verbose_logging: false,
        }
    }
}

/// Guardian agent state tracking
#[derive(Debug, Clone, Default)]
pub struct GuardianAgentState {
    /// Whether agent is actively monitoring
    pub monitoring: bool,

    /// Number of recovery requests processed
    pub recovery_requests_processed: usize,

    /// Number of messages sent
    pub messages_sent: usize,

    /// Last action timestamp
    pub last_action_time: Option<std::time::Instant>,

    /// Current demo phase awareness
    pub current_phase: Option<DemoPhase>,
}

impl SimulatedGuardianAgent {
    /// Create new simulated guardian agent
    pub async fn new(name: String, config: GuardianAgentConfig) -> anyhow::Result<Self> {
        // Create deterministic identifiers (placeholder until proper fixtures are wired)
        let device_id = DeviceId::new();
        let authority_id = AuthorityId::new();

        // Create simulation environment (use async version to avoid nested runtime)
        let environment =
            SimulationEffectComposer::for_simulation_async(device_id, config.seed).await?;

        tracing::info!(
            "Created simulated guardian agent: {} (device: {}, authority: {})",
            name,
            device_id,
            authority_id
        );

        Ok(Self {
            name,
            authority_id,
            device_id,
            environment: Arc::new(Mutex::new(environment)),
            config,
            state: GuardianAgentState::default(),
        })
    }

    /// Start the agent's autonomous behavior
    pub async fn start(&mut self) -> anyhow::Result<()> {
        self.state.monitoring = true;
        tracing::info!("Guardian agent {} started monitoring", self.name);

        // Start background tasks for autonomous behavior
        self.start_message_generation().await?;
        self.start_recovery_monitoring().await?;

        Ok(())
    }

    /// Stop the agent
    pub async fn stop(&mut self) -> anyhow::Result<()> {
        self.state.monitoring = false;
        tracing::info!("Guardian agent {} stopped", self.name);
        Ok(())
    }

    /// Process a demo event that requires guardian response
    pub async fn process_demo_event(
        &mut self,
        event: &DemoEvent,
        demo_state: &DemoState,
    ) -> anyhow::Result<Vec<DemoEvent>> {
        if !self.state.monitoring {
            return Ok(vec![]);
        }

        let mut response_events = Vec::new();

        match event {
            DemoEvent::InitiateRecovery => {
                response_events.extend(self.handle_recovery_request(demo_state).await?);
            }
            DemoEvent::AdvancePhase => {
                self.state.current_phase = Some(demo_state.phase.clone());
                response_events.extend(self.handle_phase_change(demo_state).await?);
            }
            DemoEvent::SendMessage(content) => {
                response_events.extend(self.handle_message_received(content, demo_state).await?);
            }
            _ => {
                // Other events may not require guardian response
            }
        }

        Ok(response_events)
    }

    /// Handle recovery request from Bob
    async fn handle_recovery_request(
        &mut self,
        demo_state: &DemoState,
    ) -> anyhow::Result<Vec<DemoEvent>> {
        tracing::info!("Guardian {} handling recovery request", self.name);

        // Simulate realistic response delay
        let delay_ms = self.calculate_response_delay();
        PhysicalTimeHandler::new().sleep_ms(delay_ms).await.ok();

        // Determine if this guardian will approve (based on config)
        let will_approve = self.should_approve_recovery(demo_state);

        if will_approve {
            self.state.recovery_requests_processed += 1;
            tracing::info!("Guardian {} approved recovery request", self.name);

            // Execute recovery approval through simulator
            self.execute_recovery_approval(demo_state).await?;

            Ok(vec![DemoEvent::GuardianApproval(self.authority_id)])
        } else {
            tracing::info!("Guardian {} declined recovery request", self.name);
            Ok(vec![])
        }
    }

    /// Handle demo phase changes
    async fn handle_phase_change(
        &mut self,
        demo_state: &DemoState,
    ) -> anyhow::Result<Vec<DemoEvent>> {
        let mut events = Vec::new();

        match demo_state.phase {
            DemoPhase::GuardianSetup => {
                // Automatically participate in guardian setup
                events.extend(self.participate_in_guardian_setup().await?);
            }
            DemoPhase::NormalOperation => {
                // Start normal guardian activity
                events.extend(self.start_normal_activity().await?);
            }
            DemoPhase::DeviceLoss => {
                // Acknowledge Bob's situation
                events.extend(self.acknowledge_device_loss().await?);
            }
            _ => {}
        }

        Ok(events)
    }

    /// Handle message from Bob
    async fn handle_message_received(
        &mut self,
        content: &str,
        _demo_state: &DemoState,
    ) -> anyhow::Result<Vec<DemoEvent>> {
        // Generate contextual response based on message content
        let response = self.generate_contextual_response(content).await?;

        if let Some(response_content) = response {
            self.state.messages_sent += 1;
            Ok(vec![DemoEvent::SendMessage(response_content)])
        } else {
            Ok(vec![])
        }
    }

    /// Execute recovery approval through simulator
    async fn execute_recovery_approval(&mut self, _demo_state: &DemoState) -> anyhow::Result<()> {
        tracing::debug!(
            "Guardian {} executed recovery approval via simulator",
            self.name
        );
        Ok(())
    }

    /// Participate in guardian setup
    async fn participate_in_guardian_setup(&mut self) -> anyhow::Result<Vec<DemoEvent>> {
        tracing::info!("Guardian {} participating in setup", self.name);

        Ok(vec![])
    }

    /// Start normal guardian activity
    async fn start_normal_activity(&mut self) -> anyhow::Result<Vec<DemoEvent>> {
        tracing::info!("Guardian {} starting normal activity", self.name);

        // Start periodic heartbeat and status updates
        let name = self.name.clone();
        let _authority_id = self.authority_id;

        tokio::spawn(async move {
            loop {
                PhysicalTimeHandler::new().sleep_ms(30_000).await.ok();
                tracing::debug!("Guardian {} heartbeat", name);
                // Could send status updates here
            }
        });

        Ok(vec![])
    }

    /// Acknowledge Bob's device loss
    async fn acknowledge_device_loss(&mut self) -> anyhow::Result<Vec<DemoEvent>> {
        tracing::info!("Guardian {} acknowledging Bob's device loss", self.name);

        // Generate sympathetic response
        let responses = [
            "Bob, we're here to help you recover your account.",
            "Don't worry Bob, this is exactly why we set up guardian recovery.",
            "I'm ready to help with your recovery process.",
        ];

        let response = responses[self.config.seed as usize % responses.len()].to_string();

        Ok(vec![DemoEvent::SendMessage(response)])
    }

    /// Generate contextual response to Bob's messages
    async fn generate_contextual_response(&self, content: &str) -> anyhow::Result<Option<String>> {
        // Simple contextual response generation based on keywords
        let content_lower = content.to_lowercase();

        let response = if content_lower.contains("help") {
            Some(format!("I'm here to help, Bob! - {}", self.name))
        } else if content_lower.contains("lost") || content_lower.contains("device") {
            Some(format!(
                "We'll help you recover everything, Bob. - {}",
                self.name
            ))
        } else if content_lower.contains("thank") {
            Some(format!(
                "You're welcome, Bob! That's what guardians are for. - {}",
                self.name
            ))
        } else if content_lower.contains("recovery") {
            Some(format!(
                "Your recovery is our priority, Bob. - {}",
                self.name
            ))
        } else {
            // Random friendly responses
            let friendly_responses = [
                "That's interesting, Bob!",
                "Good to hear from you!",
                "Absolutely!",
                "I agree with that.",
            ];

            // 30% chance of responding to avoid overwhelming chat
            if (self.config.seed + content.len() as u64) % 10 < 3 {
                Some(format!(
                    "{} - {}",
                    friendly_responses
                        [(self.config.seed % friendly_responses.len() as u64) as usize],
                    self.name
                ))
            } else {
                None
            }
        };

        Ok(response)
    }

    /// Start background message generation
    async fn start_message_generation(&mut self) -> anyhow::Result<()> {
        // Spawn background task for occasional autonomous messages
        let name = self.name.clone();
        let frequency = self.config.message_frequency_ms;

        tokio::spawn(async move {
            loop {
                PhysicalTimeHandler::new().sleep_ms(frequency).await.ok();

                // Occasionally send autonomous messages during normal operation
                // This would integrate with demo event system in full implementation
                tracing::debug!("Guardian {} autonomous message check", name);
            }
        });

        Ok(())
    }

    /// Start background recovery monitoring
    async fn start_recovery_monitoring(&mut self) -> anyhow::Result<()> {
        // Monitor for recovery-related events and respond appropriately
        let name = self.name.clone();

        tokio::spawn(async move {
            loop {
                PhysicalTimeHandler::new().sleep_ms(5_000).await.ok();
                tracing::trace!("Guardian {} recovery monitoring", name);
                // Monitor recovery state and respond as needed
            }
        });

        Ok(())
    }

    /// Calculate realistic response delay
    fn calculate_response_delay(&self) -> u64 {
        let (min, max) = self.config.response_delay_ms;
        let range = max - min;
        let offset = self.config.seed % range;
        min + offset
    }

    /// Determine if should approve recovery based on config and context
    fn should_approve_recovery(&self, _demo_state: &DemoState) -> bool {
        // For demo purposes, use high approval probability
        // In real implementation, would analyze recovery legitimacy
        let random_factor = (self.config.seed % 100) as f64 / 100.0;
        random_factor < self.config.approval_probability
    }

    /// Register as guardian in the system
    pub async fn register_as_guardian(&mut self) -> anyhow::Result<()> {
        tracing::info!("Registering {} as guardian", self.name);

        let environment = self.environment.lock().await;

        // Use simulation environment to register as guardian
        // This would integrate with actual guardian registration protocol
        let _effect_system = environment.effect_system();

        // Execute guardian registration through effect system
        // Placeholder for actual implementation
        tracing::debug!("Guardian {} registration executed", self.name);

        Ok(())
    }

    /// Process pending actions autonomously
    pub async fn process_pending_actions(&mut self) -> anyhow::Result<()> {
        if !self.state.monitoring {
            return Ok(());
        }

        // Process any pending scenarios or actions
        #[allow(clippy::disallowed_methods)]
        {
            self.state.last_action_time = Some(Instant::now());
        }

        Ok(())
    }

    /// Get agent statistics
    pub fn get_statistics(&self) -> GuardianAgentStatistics {
        GuardianAgentStatistics {
            name: self.name.clone(),
            authority_id: self.authority_id,
            device_id: self.device_id,
            monitoring: self.state.monitoring,
            recovery_requests_processed: self.state.recovery_requests_processed,
            messages_sent: self.state.messages_sent,
            current_phase: self.state.current_phase.clone(),
            uptime: self
                .state
                .last_action_time
                .map(|t| t.elapsed())
                .unwrap_or_default(),
        }
    }

    /// Get guardian ID for external reference
    pub fn authority_id(&self) -> AuthorityId {
        self.authority_id
    }

    /// Get guardian name
    pub fn name(&self) -> &str {
        &self.name
    }
}

/// Guardian agent statistics
#[derive(Debug, Clone)]
pub struct GuardianAgentStatistics {
    /// Guardian name
    pub name: String,
    /// Guardian authority identifier
    pub authority_id: AuthorityId,
    /// Guardian device identifier
    pub device_id: DeviceId,
    /// Whether guardian is currently monitoring
    pub monitoring: bool,
    /// Number of recovery requests processed
    pub recovery_requests_processed: usize,
    /// Number of messages sent
    pub messages_sent: usize,
    /// Current demo phase
    pub current_phase: Option<DemoPhase>,
    /// Guardian uptime duration
    pub uptime: std::time::Duration,
}

/// Factory for creating guardian agent pairs
pub struct GuardianAgentFactory;

impl GuardianAgentFactory {
    /// Create Alice and Charlie agents for demo
    pub async fn create_demo_guardians(
        seed: u64,
    ) -> anyhow::Result<(SimulatedGuardianAgent, SimulatedGuardianAgent)> {
        let alice_config = GuardianAgentConfig {
            seed,
            response_delay_ms: (2000, 4000), // Alice is relatively quick
            approval_probability: 0.98,      // Very reliable
            enable_faults: false,
            message_frequency_ms: 15000,
            verbose_logging: true,
        };

        let charlie_config = GuardianAgentConfig {
            seed: seed + 1,
            response_delay_ms: (3000, 6000), // Charlie is more deliberate
            approval_probability: 0.95,      // Also reliable
            enable_faults: false,
            message_frequency_ms: 20000,
            verbose_logging: true,
        };

        let alice = SimulatedGuardianAgent::new("Alice".to_string(), alice_config).await?;
        let charlie = SimulatedGuardianAgent::new("Charlie".to_string(), charlie_config).await?;

        Ok((alice, charlie))
    }

    /// Create guardian agents with custom configs
    pub async fn create_custom_guardians(
        configs: Vec<(String, GuardianAgentConfig)>,
    ) -> anyhow::Result<Vec<SimulatedGuardianAgent>> {
        let mut agents = Vec::new();

        for (name, config) in configs {
            let agent = SimulatedGuardianAgent::new(name, config).await?;
            agents.push(agent);
        }

        Ok(agents)
    }
}

impl std::fmt::Debug for SimulatedGuardianAgent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SimulatedGuardianAgent")
            .field("name", &self.name)
            .field("authority_id", &self.authority_id)
            .field("device_id", &self.device_id)
            .field("config", &self.config)
            .field("state", &self.state)
            .field("environment", &"<ComposedSimulationEnvironment>")
            .finish()
    }
}
