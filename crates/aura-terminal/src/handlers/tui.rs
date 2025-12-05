//! # TUI Handler
//!
//! Handler for launching the production TUI (Terminal User Interface).

use std::env;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::Result;
use serde::{Deserialize, Serialize};
// Import app types from aura-app (pure layer)
use aura_app::{AppConfig, AppCore};
// Import agent types from aura-agent (runtime layer)
use aura_agent::core::config::StorageConfig;
use aura_agent::{AgentBuilder, AgentConfig, EffectContext};
use aura_core::identifiers::{AuthorityId, ContextId};
use aura_core::AuraError;
use tokio::sync::RwLock;

use crate::cli::tui::TuiArgs;
use crate::tui::{
    context::IoContext,
    effects::EffectBridge,
    screens::{run_app, run_app_with_context},
};

/// Account configuration stored on disk
#[derive(Debug, Clone, Serialize, Deserialize)]
struct AccountConfig {
    /// The authority ID for this account
    authority_id: String,
    /// The primary context ID for this account
    context_id: String,
    /// Account creation timestamp (ms since epoch)
    created_at: u64,
}

/// Load or create account configuration
///
/// Checks if `<base_path>/account.json` exists:
/// - If yes: loads authority/context from it
/// - If no: creates new account with deterministic IDs based on device_id
fn load_or_create_account(
    base_path: &Path,
    device_id_str: &str,
) -> Result<(AuthorityId, ContextId), AuraError> {
    let account_path = base_path.join("account.json");

    if account_path.exists() {
        // Load existing account
        let content = std::fs::read_to_string(&account_path)
            .map_err(|e| AuraError::internal(format!("Failed to read account config: {}", e)))?;

        let config: AccountConfig = serde_json::from_str(&content)
            .map_err(|e| AuraError::internal(format!("Failed to parse account config: {}", e)))?;

        // Parse authority ID from hex string
        let authority_bytes: [u8; 32] = hex::decode(&config.authority_id)
            .map_err(|e| AuraError::internal(format!("Invalid authority_id hex: {}", e)))?
            .try_into()
            .map_err(|_| AuraError::internal("Invalid authority_id length"))?;
        let authority_id = AuthorityId::new_from_entropy(authority_bytes);

        // Parse context ID from hex string
        let context_bytes: [u8; 32] = hex::decode(&config.context_id)
            .map_err(|e| AuraError::internal(format!("Invalid context_id hex: {}", e)))?
            .try_into()
            .map_err(|_| AuraError::internal("Invalid context_id length"))?;
        let context_id = ContextId::new_from_entropy(context_bytes);

        println!("Loaded existing account from {}", account_path.display());
        Ok((authority_id, context_id))
    } else {
        // Create new account with deterministic IDs based on device_id
        // This ensures the same device_id always creates the same account
        let authority_entropy =
            aura_core::hash::hash(format!("authority:{}", device_id_str).as_bytes());
        let context_entropy =
            aura_core::hash::hash(format!("context:{}", device_id_str).as_bytes());

        let authority_id = AuthorityId::new_from_entropy(authority_entropy);
        let context_id = ContextId::new_from_entropy(context_entropy);

        // Save the new account configuration
        let config = AccountConfig {
            authority_id: hex::encode(authority_entropy),
            context_id: hex::encode(context_entropy),
            created_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0),
        };

        // Ensure directory exists
        if let Some(parent) = account_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                AuraError::internal(format!("Failed to create data directory: {}", e))
            })?;
        }

        let content = serde_json::to_string_pretty(&config).map_err(|e| {
            AuraError::internal(format!("Failed to serialize account config: {}", e))
        })?;

        std::fs::write(&account_path, content)
            .map_err(|e| AuraError::internal(format!("Failed to write account config: {}", e)))?;

        println!("Created new account at {}", account_path.display());
        Ok((authority_id, context_id))
    }
}

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

    // Determine device ID string for account derivation
    let device_id_for_account = device_id_str.unwrap_or("tui:production-device");

    // Load or create account configuration
    let (authority_id, context_id) = load_or_create_account(&base_path, device_id_for_account)?;
    println!("Authority: {}", authority_id);
    println!("Context: {}", context_id);

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
        authority_id,
        context_id,
        aura_core::effects::ExecutionMode::Production,
    );

    // Build production agent using AgentBuilder
    let agent = AgentBuilder::new()
        .with_config(agent_config)
        .with_authority(authority_id)
        .build_production(&effect_ctx)
        .await
        .map_err(|e| AuraError::internal(format!("Failed to create agent: {}", e)))?;

    // Create AppCore with runtime bridge - the portable application core from aura-app
    // This provides intent-based state management with reactive ViewState
    // The agent implements RuntimeBridge, enabling the dependency inversion
    let journal_path = base_path.join("journal.json");
    let app_config = AppConfig {
        data_dir: base_path.to_string_lossy().to_string(),
        debug: false,
        journal_path: Some(journal_path.to_string_lossy().to_string()),
    };
    let agent = Arc::new(agent);
    let mut app_core = AppCore::with_runtime(app_config, agent.clone().as_runtime_bridge())
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

    println!("AppCore initialized (with runtime bridge)");

    // Create effect bridge for TUI with agent and AppCore integration
    // Agent provides effect system access (dependency inversion pattern)
    // AppCore provides intent-based state management
    let bridge = EffectBridge::with_agent_and_app_core(agent, app_core.clone());

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
