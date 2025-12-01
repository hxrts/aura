#![allow(deprecated)]
//! # Demo Orchestration System
//!
//! Provides complete orchestration of the human-agent demo experience,
//! integrating TUI, simulator agents, and scenario system.

use aura_core::PhysicalTimeEffects;
use aura_effects::time::PhysicalTimeHandler;
use std::sync::Arc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use tokio::sync::{broadcast, Mutex};
use uuid::Uuid;

use super::{
    human_agent::{DemoMetrics, DemoPhase, DemoState, HumanAgentDemo, HumanAgentDemoConfig},
    scenario_bridge::DemoSetupConfig,
    simulator_integration::GuardianAgentStatistics,
};

/// Complete demo orchestration system
pub struct DemoOrchestrator {
    /// Demo configuration
    config: DemoOrchestratorConfig,

    /// Current demo session (if running)
    active_demo: Option<Arc<Mutex<HumanAgentDemo>>>,

    /// Demo event broadcast channel
    event_broadcast: broadcast::Sender<DemoOrchestratorEvent>,

    /// Demo statistics tracking
    statistics: Arc<Mutex<DemoOrchestratorStatistics>>,

    /// Demo session history
    session_history: Vec<DemoSessionRecord>,
}

/// Configuration for the demo orchestrator
#[derive(Debug, Clone)]
pub struct DemoOrchestratorConfig {
    /// Deterministic seed for reproducible demos
    pub seed: u64,

    /// Setup configuration for scenario bridge
    pub setup_config: DemoSetupConfig,

    /// Demo execution configuration
    pub demo_config: HumanAgentDemoConfig,

    /// Enable demo session recording
    pub record_sessions: bool,

    /// Maximum concurrent demo sessions
    pub max_concurrent_sessions: usize,

    /// Enable metrics collection
    pub collect_metrics: bool,

    /// Demo timeout (auto-complete after duration)
    pub demo_timeout_minutes: u64,
}

impl Default for DemoOrchestratorConfig {
    fn default() -> Self {
        Self {
            seed: 42,
            setup_config: DemoSetupConfig::default(),
            demo_config: HumanAgentDemoConfig::default(),
            record_sessions: true,
            max_concurrent_sessions: 1, // Usually one demo at a time
            collect_metrics: true,
            demo_timeout_minutes: 30,
        }
    }
}

/// Events from the demo orchestrator
#[derive(Debug, Clone)]
#[allow(missing_docs)]
pub enum DemoOrchestratorEvent {
    /// Demo session started
    SessionStarted {
        session_id: Uuid,
        participants: Vec<String>,
    },

    /// Demo phase changed
    PhaseChanged {
        session_id: Uuid,
        old_phase: DemoPhase,
        new_phase: DemoPhase,
    },

    /// Guardian action performed
    GuardianAction {
        session_id: Uuid,
        guardian_name: String,
        action: String,
    },

    /// Recovery process event
    RecoveryEvent {
        session_id: Uuid,
        event_type: String,
        details: String,
    },

    /// Demo session completed
    SessionCompleted {
        session_id: Uuid,
        duration: std::time::Duration,
        success: bool,
    },

    /// Demo session failed
    SessionFailed { session_id: Uuid, error: String },
}

/// Orchestrator statistics
#[derive(Debug, Default, Clone)]
pub struct DemoOrchestratorStatistics {
    /// Total sessions run
    pub total_sessions: usize,

    /// Successful sessions
    pub successful_sessions: usize,

    /// Failed sessions
    pub failed_sessions: usize,

    /// Total demo time
    pub total_demo_time: std::time::Duration,

    /// Average session duration
    pub average_session_duration: std::time::Duration,

    /// Total recovery operations
    pub total_recovery_operations: usize,

    /// Total guardian approvals
    pub total_guardian_approvals: usize,

    /// Guardian response statistics
    pub guardian_statistics: Vec<GuardianAgentStatistics>,
}

/// Record of a demo session
#[derive(Debug, Clone)]
pub struct DemoSessionRecord {
    /// Session identifier
    pub session_id: Uuid,

    /// Session start time
    pub start_time_ms: u64,

    /// Session duration
    pub duration: std::time::Duration,

    /// Final demo state
    pub final_state: DemoState,

    /// Success status
    pub success: bool,

    /// Error message (if failed)
    pub error: Option<String>,

    /// Session metrics
    pub metrics: DemoMetrics,

    /// Guardian statistics
    pub guardian_stats: Vec<GuardianAgentStatistics>,
}

impl DemoOrchestrator {
    /// Create new demo orchestrator
    pub fn new(config: DemoOrchestratorConfig) -> Self {
        let (event_tx, _) = broadcast::channel(100);

        Self {
            config,
            active_demo: None,
            event_broadcast: event_tx,
            statistics: Arc::new(Mutex::new(DemoOrchestratorStatistics::default())),
            session_history: Vec::new(),
        }
    }

    /// Start a new demo session
    pub async fn start_demo_session(&mut self) -> anyhow::Result<Uuid> {
        if self.active_demo.is_some() {
            return Err(anyhow::anyhow!("Demo session already running"));
        }

        let session_id = crate::ids::uuid(&format!("demo-session:{}", self.config.seed));
        tracing::info!("Starting demo session: {}", session_id);

        // Create and configure the human-agent demo
        let demo = HumanAgentDemo::new(self.config.demo_config.clone()).await?;
        self.active_demo = Some(Arc::new(Mutex::new(demo)));

        // Broadcast session started event
        let _ = self
            .event_broadcast
            .send(DemoOrchestratorEvent::SessionStarted {
                session_id,
                participants: vec![
                    "Bob".to_string(),
                    "Alice".to_string(),
                    "Charlie".to_string(),
                ],
            });

        // Start background monitoring
        self.start_session_monitoring(session_id).await?;

        tracing::info!("Demo session {} started successfully", session_id);
        Ok(session_id)
    }

    /// Run the active demo session
    pub async fn run_active_session(&mut self) -> anyhow::Result<DemoSessionRecord> {
        let demo = self
            .active_demo
            .take()
            .ok_or_else(|| anyhow::anyhow!("No active demo session"))?;

        let session_id = crate::ids::uuid(&format!("demo-session:completion:{}", self.config.seed));
        #[allow(clippy::disallowed_methods)]
        let start_time_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        #[allow(clippy::disallowed_methods)]
        let start_instant = Instant::now();

        // Run the demo
        let result = {
            let mut demo_guard = demo.lock().await;
            demo_guard.run().await
        };

        let duration = start_instant.elapsed();

        // Create session record
        let session_record = match result {
            Ok(()) => {
                // Update statistics for successful session
                self.update_success_statistics(duration).await;

                let _ = self
                    .event_broadcast
                    .send(DemoOrchestratorEvent::SessionCompleted {
                        session_id,
                        duration,
                        success: true,
                    });

                DemoSessionRecord {
                    session_id,
                    start_time_ms,
                    duration,
                    final_state: DemoState::default(), // Would extract from demo
                    success: true,
                    error: None,
                    metrics: DemoMetrics::default(), // Would extract from demo
                    guardian_stats: Vec::new(),      // Would extract from demo
                }
            }
            Err(e) => {
                // Update statistics for failed session
                self.update_failure_statistics(duration).await;

                let error_msg = e.to_string();
                let _ = self
                    .event_broadcast
                    .send(DemoOrchestratorEvent::SessionFailed {
                        session_id,
                        error: error_msg.clone(),
                    });

                DemoSessionRecord {
                    session_id,
                    start_time_ms,
                    duration,
                    final_state: DemoState::default(),
                    success: false,
                    error: Some(error_msg),
                    metrics: DemoMetrics::default(),
                    guardian_stats: Vec::new(),
                }
            }
        };

        // Record session if enabled
        if self.config.record_sessions {
            self.session_history.push(session_record.clone());
        }

        Ok(session_record)
    }

    /// Stop the active demo session
    pub async fn stop_active_session(&mut self) -> anyhow::Result<()> {
        if let Some(demo) = self.active_demo.take() {
            tracing::info!("Stopping active demo session");
            // Demo will stop when dropped
            std::mem::drop(demo);
        }

        Ok(())
    }

    /// Get current demo statistics
    pub async fn get_statistics(&self) -> DemoOrchestratorStatistics {
        self.statistics.lock().await.clone()
    }

    /// Get session history
    pub fn get_session_history(&self) -> &[DemoSessionRecord] {
        &self.session_history
    }

    /// Subscribe to orchestrator events
    pub fn subscribe_to_events(&self) -> broadcast::Receiver<DemoOrchestratorEvent> {
        self.event_broadcast.subscribe()
    }

    /// Start background session monitoring
    async fn start_session_monitoring(&self, session_id: Uuid) -> anyhow::Result<()> {
        let timeout = std::time::Duration::from_secs(self.config.demo_timeout_minutes * 60);
        let event_sender = self.event_broadcast.clone();

        tokio::spawn(async move {
            let time = PhysicalTimeHandler::new();
            let _ = time.sleep_ms(timeout.as_millis() as u64).await;

            tracing::warn!("Demo session {} timed out", session_id);
            let _ = event_sender.send(DemoOrchestratorEvent::SessionFailed {
                session_id,
                error: "Session timed out".to_string(),
            });
        });

        Ok(())
    }

    /// Update statistics for successful session
    async fn update_success_statistics(&self, duration: std::time::Duration) {
        let mut stats = self.statistics.lock().await;
        stats.total_sessions += 1;
        stats.successful_sessions += 1;
        stats.total_demo_time += duration;

        // Calculate average duration
        if stats.total_sessions > 0 {
            stats.average_session_duration = stats.total_demo_time / stats.total_sessions as u32;
        }
    }

    /// Update statistics for failed session
    async fn update_failure_statistics(&self, duration: std::time::Duration) {
        let mut stats = self.statistics.lock().await;
        stats.total_sessions += 1;
        stats.failed_sessions += 1;
        stats.total_demo_time += duration;

        // Calculate average duration
        if stats.total_sessions > 0 {
            stats.average_session_duration = stats.total_demo_time / stats.total_sessions as u32;
        }
    }

    /// Check if demo session is running
    pub fn is_session_active(&self) -> bool {
        self.active_demo.is_some()
    }

    /// Get current session statistics summary
    pub async fn get_session_summary(&self) -> anyhow::Result<String> {
        let stats = self.get_statistics().await;

        Ok(format!(
            "Demo Sessions Summary:\n\
             Total Sessions: {}\n\
             Successful: {}\n\
             Failed: {}\n\
             Success Rate: {:.1}%\n\
             Average Duration: {:.1}s\n\
             Total Demo Time: {:.1}s",
            stats.total_sessions,
            stats.successful_sessions,
            stats.failed_sessions,
            if stats.total_sessions > 0 {
                (stats.successful_sessions as f64 / stats.total_sessions as f64) * 100.0
            } else {
                0.0
            },
            stats.average_session_duration.as_secs_f64(),
            stats.total_demo_time.as_secs_f64()
        ))
    }
}

/// Builder for demo orchestrator
#[derive(Default)]
pub struct DemoOrchestratorBuilder {
    config: DemoOrchestratorConfig,
}

impl DemoOrchestratorBuilder {
    /// Create new builder
    pub fn new() -> Self {
        Self::default()
    }

    /// Set deterministic seed
    pub fn with_seed(mut self, seed: u64) -> Self {
        self.config.seed = seed;
        self.config.demo_config.seed = seed;
        self
    }

    /// Set setup configuration
    pub fn with_setup_config(mut self, config: DemoSetupConfig) -> Self {
        self.config.setup_config = config;
        self
    }

    /// Set demo configuration
    pub fn with_demo_config(mut self, config: HumanAgentDemoConfig) -> Self {
        self.config.demo_config = config;
        self
    }

    /// Enable session recording
    pub fn with_session_recording(mut self, enabled: bool) -> Self {
        self.config.record_sessions = enabled;
        self
    }

    /// Set demo timeout
    pub fn with_timeout_minutes(mut self, minutes: u64) -> Self {
        self.config.demo_timeout_minutes = minutes;
        self
    }

    /// Build the orchestrator
    pub fn build(self) -> DemoOrchestrator {
        DemoOrchestrator::new(self.config)
    }
}

/// High-level demo execution function
pub async fn execute_complete_demo(
    seed: Option<u64>,
    verbose: bool,
) -> anyhow::Result<DemoSessionRecord> {
    let seed = seed.unwrap_or(42);

    tracing::info!("Executing complete human-agent demo (seed: {})", seed);

    // Configure demo with appropriate settings
    let setup_config = DemoSetupConfig {
        participant_count: 3,
        guardian_threshold: 2,
        setup_chat_history: true,
        initial_message_count: 5,
        verbose_logging: verbose,
        simulate_network_activity: true,
    };

    let demo_config = HumanAgentDemoConfig {
        auto_advance: true,
        agent_delay_ms: if verbose { 2000 } else { 500 }, // Slower for verbose demo
        verbose_logging: verbose,
        guardian_response_time_ms: 3000,
        max_demo_duration_minutes: 15,
        seed,
    };

    // Create and run orchestrator
    let mut orchestrator = DemoOrchestratorBuilder::new()
        .with_seed(seed)
        .with_setup_config(setup_config)
        .with_demo_config(demo_config)
        .with_session_recording(true)
        .with_timeout_minutes(20)
        .build();

    // Start and run demo session
    let session_id = orchestrator.start_demo_session().await?;
    tracing::info!("Demo session started: {}", session_id);

    let session_record = orchestrator.run_active_session().await?;

    // Print summary
    let summary = orchestrator.get_session_summary().await?;
    println!("\n{}", summary);

    if session_record.success {
        println!("Demo completed.");
    } else {
        let error_msg = session_record
            .error
            .clone()
            .unwrap_or_else(|| "Unknown error".to_string());
        println!("Demo failed: {}", error_msg);
    }

    Ok(session_record)
}

/// Demo orchestrator CLI interface
pub struct DemoOrchestratorCli {
    orchestrator: DemoOrchestrator,
}

impl DemoOrchestratorCli {
    /// Create new CLI interface
    pub fn new(config: DemoOrchestratorConfig) -> Self {
        Self {
            orchestrator: DemoOrchestrator::new(config),
        }
    }

    /// Run interactive demo CLI
    pub async fn run_interactive(&mut self) -> anyhow::Result<()> {
        println!("Aura Human-Agent Demo Orchestrator");
        println!("===================================");

        // Run a single demo session
        let session_id = self.orchestrator.start_demo_session().await?;
        println!("Started demo session: {}", session_id);

        let record = self.orchestrator.run_active_session().await?;
        if record.success {
            println!(
                "Demo completed successfully in {:.1}s!",
                record.duration.as_secs_f64()
            );
        } else {
            println!(
                "Demo failed: {}",
                record.error.unwrap_or("Unknown".to_string())
            );
        }

        Ok(())
    }
}
