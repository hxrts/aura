//! # Demo Module
//!
//! Provides demo modes for showcasing Aura's capabilities.


pub mod human_agent;
pub mod orchestration;
pub mod scenario_bridge;
pub mod simulator_integration;

pub use human_agent::{
    DemoMetrics, DemoPhase, DemoState, HumanAgentDemo, HumanAgentDemoBuilder, HumanAgentDemoConfig,
};

pub use simulator_integration::{
    GuardianAgentConfig, GuardianAgentFactory, GuardianAgentState, GuardianAgentStatistics,
    SimulatedGuardianAgent,
};

pub use scenario_bridge::{
    setup_and_run_human_agent_demo, DemoScenarioBridge, DemoScenarioBridgeBuilder, DemoSetupConfig,
    DemoSetupResult, SetupMetrics,
};

pub use orchestration::{
    execute_complete_demo, DemoOrchestrator, DemoOrchestratorBuilder, DemoOrchestratorCli,
    DemoOrchestratorConfig, DemoOrchestratorEvent, DemoOrchestratorStatistics, DemoSessionRecord,
};
