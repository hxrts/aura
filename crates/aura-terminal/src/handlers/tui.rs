//! # TUI Handler
//!
//! Handler for launching the production TUI (Terminal User Interface).

use std::env;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
// Import from aura-app which re-exports agent types
use aura_app::{AgentBuilder, AgentConfig, AppConfig, AppCore, EffectContext, StorageConfig};
use aura_core::identifiers::{AuthorityId, ContextId};
use aura_core::AuraError;
use tokio::sync::RwLock;

use crate::cli::tui::TuiArgs;
use crate::tui::{
    context::IoContext,
    effects::EffectBridge,
    screens::{run_app, run_app_with_context},
};

/// Handle TUI launch
pub async fn handle_tui(args: &TuiArgs) -> Result<()> {
    // Demo mode uses static sample data
    if args.demo {
        println!("Starting Aura TUI (Demo Mode)");
        println!("=============================");
        println!("Using sample data for demonstration");
        println!();
        return run_app()
            .await
            .map_err(|e| AuraError::internal(format!("TUI failed: {}", e)).into());
    }

    let data_dir = args.data_dir.clone().or_else(|| env::var("AURA_PATH").ok());
    handle_tui_launch(data_dir.as_deref(), args.device_id.as_deref()).await
}

/// Launch the production TUI
async fn handle_tui_launch(data_dir: Option<&str>, device_id_str: Option<&str>) -> Result<()> {
    println!("Starting Aura TUI");
    println!("================");

    // Determine data directory
    let base_path = data_dir
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("./aura-data"));

    // Determine device ID
    let device_id = device_id_str
        .map(|id| crate::ids::device_id(id))
        .unwrap_or_else(|| crate::ids::device_id("tui:production-device"));

    println!("Data directory: {}", base_path.display());
    println!("Device ID: {}", device_id);

    // Create agent configuration for production TUI
    let agent_config = AgentConfig {
        device_id,
        storage: StorageConfig {
            base_path: base_path.clone(),
            ..StorageConfig::default()
        },
        ..AgentConfig::default()
    };

    // Create effect context for agent initialization
    let effect_ctx = EffectContext::new(
        AuthorityId::new_from_entropy([0u8; 32]), // Temporary authority, agent will update
        ContextId::new_from_entropy([0u8; 32]),   // Temporary context
        aura_core::effects::ExecutionMode::Production,
    );

    // Build production agent using AgentBuilder
    // This creates the AuraAgent with full effect system and services
    let agent = AgentBuilder::new()
        .with_config(agent_config)
        .with_authority(AuthorityId::new_from_entropy([0u8; 32])) // Will be replaced by loaded account
        .build_production(&effect_ctx)
        .await
        .map_err(|e| AuraError::internal(format!("Failed to create agent: {}", e)))?;

    // Create AppCore with agent - the portable application core from aura-app
    // This provides intent-based state management with reactive ViewState
    // The agent is now wrapped inside AppCore, providing a unified interface
    let journal_path = base_path.join("journal.json");
    let app_config = AppConfig {
        data_dir: base_path.to_string_lossy().to_string(),
        debug: false,
        journal_path: Some(journal_path.to_string_lossy().to_string()),
    };
    let mut app_core = AppCore::with_agent(app_config, agent)
        .map_err(|e| AuraError::internal(format!("Failed to create AppCore: {}", e)))?;

    // Load existing journal facts from storage to rebuild ViewState
    match app_core.load_from_storage(&journal_path) {
        Ok(count) if count > 0 => {
            println!("Loaded {} facts from journal", count);
        }
        Ok(_) => {
            println!("No existing journal found, starting fresh");
        }
        Err(e) => {
            eprintln!("Warning: Failed to load journal: {} - starting fresh", e);
        }
    }

    let app_core = Arc::new(RwLock::new(app_core));

    println!("AppCore initialized (with agent)");

    // Create effect bridge for TUI with AppCore integration
    // Effect system is accessed via app_core.agent().runtime().effects()
    let bridge = EffectBridge::with_app_core(app_core.clone());

    // Create IoContext with AppCore integration
    let ctx = IoContext::with_app_core(bridge, app_core);

    println!("TUI ready! Launching...");
    println!();

    // Run the iocraft TUI app with context
    run_app_with_context(ctx)
        .await
        .map_err(|e| AuraError::internal(format!("TUI failed: {}", e)))?;

    Ok(())
}
