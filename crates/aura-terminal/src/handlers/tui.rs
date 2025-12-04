//! # TUI Handler
//!
//! Handler for launching the production TUI (Terminal User Interface).

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use aura_agent::core::config::StorageConfig;
use aura_agent::{AgentConfig, AuraEffectSystem};
use aura_core::AuraError;
use aura_effects::time::PhysicalTimeHandler;

use crate::cli::tui::TuiArgs;
use crate::tui::{
    context::IoContext,
    effects::{BridgeConfig, EffectBridge},
    screens::run_app_with_context,
};

/// Handle TUI launch
pub async fn handle_tui(args: &TuiArgs) -> Result<()> {
    handle_tui_launch(args.data_dir.as_deref(), args.device_id.as_deref()).await
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
            base_path,
            ..StorageConfig::default()
        },
        ..AgentConfig::default()
    };

    // Build production effect system
    let effect_system = AuraEffectSystem::production(agent_config)
        .map_err(|e| AuraError::internal(format!("Failed to create effect system: {}", e)))?;
    let effect_system = Arc::new(effect_system);

    // Cast to trait objects for bridge
    let amp_trait: Option<Arc<dyn aura_core::effects::amp::AmpChannelEffects + Send + Sync>> =
        Some(effect_system.clone() as Arc<_>);

    // Create effect bridge for TUI
    let bridge = EffectBridge::with_config_time_amp_system(
        BridgeConfig::default(),
        Arc::new(PhysicalTimeHandler),
        amp_trait.clone(),
        Some(effect_system.clone()),
    );

    // Create IoContext (self-contained context for iocraft TUI)
    let ctx = IoContext::new(bridge);

    println!("TUI ready! Launching...");
    println!();

    // Run the iocraft TUI app with context
    run_app_with_context(ctx)
        .await
        .map_err(|e| AuraError::internal(format!("TUI failed: {}", e)))?;

    Ok(())
}
