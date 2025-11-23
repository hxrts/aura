//! # Demo Handler
//!
//! Handler for demo-related CLI commands.

use std::path::PathBuf;

use aura_core::AuraError;

use crate::{
    commands::demo::DemoCommands,
    demo::{
        execute_complete_demo, setup_and_run_human_agent_demo, DemoOrchestratorCli,
        DemoOrchestratorConfig, DemoScenarioBridge, DemoSetupConfig, HumanAgentDemoConfig,
    },
};

/// Handler for demo commands
pub struct DemoHandler;

impl DemoHandler {
    /// Handle demo commands
    pub async fn handle_demo_command(command: DemoCommands) -> Result<(), AuraError> {
        match command {
            DemoCommands::HumanAgent {
                seed,
                verbose,
                auto_advance,
                timeout_minutes,
                guardian_delay_ms,
                record_to,
            } => {
                Self::handle_human_agent_demo(
                    seed,
                    verbose,
                    auto_advance,
                    timeout_minutes,
                    guardian_delay_ms,
                    record_to,
                )
                .await
            }

            DemoCommands::Orchestrator {
                seed,
                record_sessions,
                max_sessions,
            } => Self::handle_orchestrator_demo(seed, record_sessions, max_sessions).await,

            DemoCommands::Stats {
                detailed,
                export_to,
            } => Self::handle_stats_command(detailed, export_to).await,

            DemoCommands::Scenario {
                config,
                participants,
                threshold,
                setup_chat,
                initial_messages,
                setup_only,
            } => {
                Self::handle_scenario_demo(
                    config,
                    participants,
                    threshold,
                    setup_chat,
                    initial_messages,
                    setup_only,
                )
                .await
            }
        }
    }

    /// Handle human-agent demo command
    async fn handle_human_agent_demo(
        seed: u64,
        verbose: bool,
        auto_advance: bool,
        timeout_minutes: u64,
        guardian_delay_ms: u64,
        record_to: Option<PathBuf>,
    ) -> Result<(), AuraError> {
        println!("Starting Human-Agent Recovery Demo");
        println!("==================================");
        println!("Seed: {}", seed);
        println!("Verbose: {}", verbose);
        println!("Auto-advance: {}", auto_advance);
        println!();

        // Configure demo
        let setup_config = DemoSetupConfig {
            participant_count: 3,
            guardian_threshold: 2,
            setup_chat_history: true,
            initial_message_count: if verbose { 8 } else { 5 },
            verbose_logging: verbose,
            simulate_network_activity: true,
        };

        let demo_config = HumanAgentDemoConfig {
            auto_advance,
            agent_delay_ms: if verbose { 2000 } else { 1000 },
            verbose_logging: verbose,
            guardian_response_time_ms: guardian_delay_ms,
            max_demo_duration_minutes: timeout_minutes,
        };

        // Execute demo
        match setup_and_run_human_agent_demo(setup_config, demo_config, seed).await {
            Ok(()) => {
                println!("\nHuman-agent demo completed successfully!");

                if let Some(path) = record_to {
                    println!("Session recording would be saved to: {:?}", path);
                    // In full implementation, would save recording
                }
            }
            Err(e) => {
                eprintln!("\nDemo failed: {}", e);
                return Err(AuraError::internal(format!(
                    "Demo execution failed: {}",
                    e
                )));
            }
        }

        Ok(())
    }

    /// Handle orchestrator demo command
    async fn handle_orchestrator_demo(
        seed: u64,
        record_sessions: bool,
        max_sessions: usize,
    ) -> Result<(), AuraError> {
        println!("Starting Demo Orchestrator");
        println!("==========================");
        println!("Seed: {}", seed);
        println!("Recording: {}", record_sessions);
        println!("Max sessions: {}", max_sessions);
        println!();

        let config = DemoOrchestratorConfig {
            seed,
            setup_config: DemoSetupConfig::default(),
            demo_config: HumanAgentDemoConfig::default(),
            record_sessions,
            max_concurrent_sessions: max_sessions,
            collect_metrics: true,
            demo_timeout_minutes: 20,
        };

        let mut cli = DemoOrchestratorCli::new(config);

        match cli.run_interactive().await {
            Ok(()) => {
                println!("\nOrchestrator session completed!");
            }
            Err(e) => {
                eprintln!("\nOrchestrator failed: {}", e);
                return Err(AuraError::internal(format!("Orchestrator failed: {}", e)));
            }
        }

        Ok(())
    }

    /// Handle stats command
    async fn handle_stats_command(
        detailed: bool,
        export_to: Option<PathBuf>,
    ) -> Result<(), AuraError> {
        println!("Demo Statistics");
        println!("===============");

        if detailed {
            println!("Detailed statistics would be shown here");
            // In full implementation, would show detailed stats
        } else {
            println!("Summary statistics would be shown here");
            // In full implementation, would show summary stats
        }

        if let Some(path) = export_to {
            println!("Statistics would be exported to: {:?}", path);
            // In full implementation, would export stats
        }

        Ok(())
    }

    /// Handle scenario demo command
    async fn handle_scenario_demo(
        config: Option<PathBuf>,
        participants: usize,
        threshold: usize,
        setup_chat: bool,
        initial_messages: usize,
        setup_only: bool,
    ) -> Result<(), AuraError> {
        println!("Setting up Scenario-Based Demo");
        println!("===============================");
        println!("Participants: {}", participants);
        println!("Threshold: {}", threshold);
        println!("Setup chat: {}", setup_chat);
        println!();

        // Configure scenario setup
        let setup_config = DemoSetupConfig {
            participant_count: participants,
            guardian_threshold: threshold,
            setup_chat_history: setup_chat,
            initial_message_count: initial_messages,
            verbose_logging: true,
            simulate_network_activity: true,
        };

        if let Some(config_path) = config {
            println!("Using config file: {:?}", config_path);
            // In full implementation, would load config from file
        }

        // Create scenario bridge
        let bridge = DemoScenarioBridge::new(42, setup_config);

        match bridge.setup_demo_environment().await {
            Ok(setup_result) => {
                println!("Scenario setup completed successfully!");
                println!(
                    "Setup took {:.2}s with {} scenarios executed",
                    setup_result.setup_metrics.setup_duration.as_secs_f64(),
                    setup_result.setup_metrics.scenarios_executed
                );

                if !setup_only {
                    println!("\nDemo environment ready - would hand off to TUI...");
                    // In full implementation, would start TUI demo
                }
            }
            Err(e) => {
                eprintln!("\nScenario setup failed: {}", e);
                return Err(AuraError::internal(format!(
                    "Scenario setup failed: {}",
                    e
                )));
            }
        }

        Ok(())
    }
}
