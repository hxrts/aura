//! # TUI Handler
//!
//! Handler for launching the TUI (Terminal User Interface).
//!
//! The TUI code is identical for production and demo modes.
//! The only difference is the backend:
//! - Production: Uses `build_production()` for real network/storage
//! - Demo: Uses `build_simulation_async()` for simulated effects

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
#[cfg(feature = "development")]
use crate::demo::{DemoSignalCoordinator, DemoSimulator};
use crate::tui::{context::IoContext, screens::run_app_with_context};

/// Whether the TUI is running in demo or production mode
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TuiMode {
    /// Production mode with real network/storage
    Production,
    /// Demo mode with simulated effects and peer agents
    Demo { seed: u64 },
}

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

/// Account loading result
pub enum AccountLoadResult {
    /// Account loaded from existing file
    Loaded {
        authority: AuthorityId,
        context: ContextId,
    },
    /// No account exists - need to show setup modal
    NotFound,
}

/// Sentinel prefix for placeholder authority IDs (used before account setup)
pub const PLACEHOLDER_AUTHORITY_PREFIX: &str = "placeholder:";

/// Try to load an existing account configuration
///
/// Returns `Loaded` if account exists, `NotFound` otherwise.
/// Does NOT auto-create accounts - this allows the TUI to show an account setup modal.
fn try_load_account(base_path: &Path) -> Result<AccountLoadResult, AuraError> {
    let account_path = base_path.join("account.json");

    if !account_path.exists() {
        return Ok(AccountLoadResult::NotFound);
    }

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
    Ok(AccountLoadResult::Loaded {
        authority: authority_id,
        context: context_id,
    })
}

/// Create placeholder authority/context IDs for pre-account-setup state
///
/// These use a deterministic sentinel value so the TUI can detect that
/// the account hasn't been set up yet (via `has_account()` check).
fn create_placeholder_ids(device_id_str: &str) -> (AuthorityId, ContextId) {
    // Use placeholder prefix so we can detect this is not a real account
    let authority_entropy = aura_core::hash::hash(
        format!(
            "{}authority:{}",
            PLACEHOLDER_AUTHORITY_PREFIX, device_id_str
        )
        .as_bytes(),
    );
    let context_entropy = aura_core::hash::hash(
        format!("{}context:{}", PLACEHOLDER_AUTHORITY_PREFIX, device_id_str).as_bytes(),
    );

    let authority_id = AuthorityId::new_from_entropy(authority_entropy);
    let context_id = ContextId::new_from_entropy(context_entropy);

    (authority_id, context_id)
}

/// Create a new account and save to disk
///
/// Called when user completes the account setup modal.
pub fn create_account(
    base_path: &Path,
    device_id_str: &str,
) -> Result<(AuthorityId, ContextId), AuraError> {
    let account_path = base_path.join("account.json");

    // Create new account with deterministic IDs based on device_id
    // This ensures the same device_id always creates the same account
    let authority_entropy =
        aura_core::hash::hash(format!("authority:{}", device_id_str).as_bytes());
    let context_entropy = aura_core::hash::hash(format!("context:{}", device_id_str).as_bytes());

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
        std::fs::create_dir_all(parent)
            .map_err(|e| AuraError::internal(format!("Failed to create data directory: {}", e)))?;
    }

    let content = serde_json::to_string_pretty(&config)
        .map_err(|e| AuraError::internal(format!("Failed to serialize account config: {}", e)))?;

    std::fs::write(&account_path, content)
        .map_err(|e| AuraError::internal(format!("Failed to write account config: {}", e)))?;

    println!("Created new account at {}", account_path.display());
    Ok((authority_id, context_id))
}

/// Demo seed for deterministic simulation
const DEMO_SEED: u64 = 2024;

/// Handle TUI launch
pub async fn handle_tui(args: &TuiArgs) -> Result<()> {
    // Demo mode: use simulation backend with deterministic seed
    // The TUI code is IDENTICAL for demo and production - only the backend differs
    if args.demo {
        println!("Starting Aura TUI (Demo Mode)");
        println!("=============================");
        println!("Demo mode runs a real agent with simulated effects.");
        println!("Seed: {} (deterministic)", DEMO_SEED);
        println!();

        // Use demo-specific data directory and device ID
        let demo_data_dir = args
            .data_dir
            .clone()
            .unwrap_or_else(|| "./aura-demo-data".to_string());
        let demo_device_id = args
            .device_id
            .clone()
            .unwrap_or_else(|| "demo:bob".to_string());

        return handle_tui_launch(
            Some(&demo_data_dir),
            Some(&demo_device_id),
            TuiMode::Demo { seed: DEMO_SEED },
        )
        .await;
    }

    let data_dir = args.data_dir.clone().or_else(|| env::var("AURA_PATH").ok());
    handle_tui_launch(
        data_dir.as_deref(),
        args.device_id.as_deref(),
        TuiMode::Production,
    )
    .await
}

/// Launch the TUI with the specified mode
async fn handle_tui_launch(
    data_dir: Option<&str>,
    device_id_str: Option<&str>,
    mode: TuiMode,
) -> Result<()> {
    let mode_str = match mode {
        TuiMode::Production => "Production",
        TuiMode::Demo { .. } => "Demo (Simulation)",
    };
    println!("Starting Aura TUI ({})", mode_str);
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

    // Try to load existing account, or use placeholders if no account exists
    let (authority_id, context_id, has_existing_account) = match try_load_account(&base_path)? {
        AccountLoadResult::Loaded { authority, context } => {
            println!("Authority: {}", authority);
            println!("Context: {}", context);
            (authority, context, true)
        }
        AccountLoadResult::NotFound => {
            // Use placeholder IDs - the TUI will show account setup modal
            let (authority, context) = create_placeholder_ids(device_id_for_account);
            println!("No existing account - will show setup modal");
            println!("Placeholder Authority: {}", authority);
            println!("Placeholder Context: {}", context);
            (authority, context, false)
        }
    };

    // Create agent configuration
    let agent_config = AgentConfig {
        device_id,
        storage: StorageConfig {
            base_path: base_path.clone(),
            ..StorageConfig::default()
        },
        ..AgentConfig::default()
    };

    // Create effect context for agent initialization
    let execution_mode = match mode {
        TuiMode::Production => aura_core::effects::ExecutionMode::Production,
        TuiMode::Demo { seed } => aura_core::effects::ExecutionMode::Simulation { seed },
    };
    let effect_ctx = EffectContext::new(authority_id, context_id, execution_mode);

    // Build agent using appropriate builder method based on mode
    let agent = match mode {
        TuiMode::Production => AgentBuilder::new()
            .with_config(agent_config)
            .with_authority(authority_id)
            .build_production(&effect_ctx)
            .await
            .map_err(|e| AuraError::internal(format!("Failed to create agent: {}", e)))?,
        TuiMode::Demo { seed } => {
            println!("Using simulation agent with seed: {}", seed);
            AgentBuilder::new()
                .with_config(agent_config)
                .with_authority(authority_id)
                .build_simulation_async(seed, &effect_ctx)
                .await
                .map_err(|e| {
                    AuraError::internal(format!("Failed to create simulation agent: {}", e))
                })?
        }
    };

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

    // Initialize reactive signals for the unified effect system
    // This registers all application signals (CHAT_SIGNAL, RECOVERY_SIGNAL, etc.)
    // with the ReactiveHandler so screens can subscribe to state changes
    {
        let core = app_core.write().await;
        if let Err(e) = core.init_signals().await {
            eprintln!(
                "Warning: Failed to initialize signals: {} - reactive updates may not work",
                e
            );
        }
    }

    println!("AppCore initialized (with runtime bridge and reactive signals)");

    // In demo mode, start the simulator with Alice and Charlie peer agents
    // DemoSignalCoordinator handles bidirectional event routing via signals
    #[cfg(feature = "development")]
    let (mut simulator, _coordinator_handles): (
        Option<DemoSimulator>,
        Option<(tokio::task::JoinHandle<()>, tokio::task::JoinHandle<()>)>,
    ) = match mode {
        TuiMode::Demo { seed } => {
            println!("Starting demo simulator...");
            let mut sim = DemoSimulator::new(seed)
                .await
                .map_err(|e| AuraError::internal(format!("Failed to create simulator: {}", e)))?;

            sim.start()
                .await
                .map_err(|e| AuraError::internal(format!("Failed to start simulator: {}", e)))?;

            let alice_id = sim.alice_authority().await;
            let charlie_id = sim.charlie_authority().await;
            println!("Alice online: {}", alice_id);
            println!("Charlie online: {}", charlie_id);
            println!("Peers connected: {}", sim.peer_count());

            // Get the simulator's bridge and response receiver for signal coordinator
            let sim_bridge = sim.bridge();
            let response_rx = sim
                .take_response_receiver()
                .await
                .ok_or_else(|| AuraError::internal("Response receiver already taken"))?;

            // Create DemoSignalCoordinator - handles bidirectional event routing via signals
            // This replaces the manual AuraEvent forwarding with signal subscriptions
            let coordinator = Arc::new(DemoSignalCoordinator::new(
                app_core.clone(),
                authority_id, // Bob's authority
                sim_bridge,
                response_rx,
            ));

            // Start coordinator tasks
            let handles = coordinator.start();
            println!("Demo signal coordinator started");

            (Some(sim), Some(handles))
        }
        TuiMode::Production => (None, None),
    };

    // Create IoContext with AppCore integration
    // Pass has_existing_account so the TUI knows whether to show the account setup modal
    // In demo mode, include hints with Alice/Charlie invite codes
    #[cfg(feature = "development")]
    let ctx = match mode {
        TuiMode::Demo { seed } => {
            let hints = crate::demo::DemoHints::new(seed);
            println!("Demo hints available:");
            println!("  Alice invite code: {}", hints.alice_invite_code);
            println!("  Charlie invite code: {}", hints.charlie_invite_code);
            IoContext::with_demo_hints(app_core, hints, has_existing_account)
        }
        TuiMode::Production => {
            IoContext::with_account_status(app_core.clone(), has_existing_account)
        }
    };

    #[cfg(not(feature = "development"))]
    let ctx = IoContext::with_account_status(app_core.clone(), has_existing_account);

    // Without development feature, demo mode just shows a warning
    #[cfg(not(feature = "development"))]
    if matches!(mode, TuiMode::Demo { .. }) {
        println!("Note: Demo mode simulation requires the 'development' feature.");
        println!("Running with simulation agent but without peer agents (Alice/Charlie).");
    }

    println!("Launching TUI...");
    println!();

    // Run the iocraft TUI app with context
    let result = run_app_with_context(ctx)
        .await
        .map_err(|e| AuraError::internal(format!("TUI failed: {}", e)));

    // In demo mode, stop the simulator cleanly
    #[cfg(feature = "development")]
    if let Some(ref mut sim) = simulator {
        println!("Stopping demo simulator...");
        if let Err(e) = sim.stop().await {
            eprintln!("Warning: Failed to stop simulator cleanly: {}", e);
        }
    }

    result?;
    Ok(())
}
