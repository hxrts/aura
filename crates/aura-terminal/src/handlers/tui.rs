//! # TUI Handler
//!
//! Handler for launching the production TUI (Terminal User Interface).

use std::env;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use aura_agent::core::config::StorageConfig;
use aura_agent::{AgentConfig, AuraEffectSystem};
use aura_app::{AppConfig, AppCore};
use aura_core::AuraError;
use aura_effects::time::PhysicalTimeHandler;
use tokio::sync::RwLock;

use crate::cli::tui::TuiArgs;
use crate::tui::{
    context::IoContext,
    effects::{BridgeConfig, EffectBridge},
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

    // Build production effect system
    let effect_system = AuraEffectSystem::production(agent_config)
        .map_err(|e| AuraError::internal(format!("Failed to create effect system: {}", e)))?;
    let effect_system = Arc::new(effect_system);

    // Create AppCore - the portable application core from aura-app
    // This provides intent-based state management with reactive ViewState
    let journal_path = base_path.join("journal.json");
    let app_config = AppConfig {
        data_dir: base_path.to_string_lossy().to_string(),
        debug: false,
        journal_path: Some(journal_path.to_string_lossy().to_string()),
    };
    let mut app_core = AppCore::new(app_config)
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

    println!("AppCore initialized");

    // Cast to trait objects for bridge
    let amp_trait: Option<Arc<dyn aura_core::effects::amp::AmpChannelEffects + Send + Sync>> =
        Some(effect_system.clone() as Arc<_>);

    // Create effect bridge for TUI with AppCore integration
    let bridge = EffectBridge::with_full_config(
        BridgeConfig::default(),
        Arc::new(PhysicalTimeHandler),
        amp_trait.clone(),
        Some(effect_system.clone()),
        Some(app_core.clone()),
    );

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
