//! # TUI Handler
//!
//! Handler for launching the TUI (Terminal User Interface).
//!
//! The TUI code is identical for production and demo modes.
//! The only difference is the backend:
//! - Production: Uses `build_production()` for real network/storage
//! - Demo: Uses `build_simulation_async()` for simulated effects

#![deny(clippy::print_stdout)]
#![deny(clippy::print_stderr)]

use std::env;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
// Import app types from aura-app (pure layer)
use aura_app::{AppConfig, AppCore};
// Import agent types from aura-agent (runtime layer)
use async_lock::RwLock;
use aura_agent::core::config::StorageConfig;
use aura_agent::{AgentBuilder, AgentConfig, EffectContext};
use aura_core::effects::time::PhysicalTimeEffects;
use aura_core::effects::StorageEffects;
use aura_core::identifiers::{AuthorityId, ContextId};
use aura_core::AuraError;
use aura_effects::time::PhysicalTimeHandler;
use aura_effects::PathFilesystemStorageHandler;
use tracing_subscriber::EnvFilter;

use crate::cli::tui::TuiArgs;
#[cfg(feature = "development")]
use crate::demo::{DemoSignalCoordinator, DemoSimulator};
use crate::handlers::tui_stdio::{during_fullscreen, PreFullscreenStdio};
use crate::tui::{
    context::{InitializedAppCore, IoContext},
    screens::run_app_with_context,
};

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
pub struct AccountConfig {
    /// The authority ID for this account
    authority_id: String,
    /// The primary context ID for this account
    context_id: String,
    /// User display name for this device
    ///
    /// This is a UI-facing profile field and is not yet persisted as a journal fact.
    /// It is stored here so terminal frontends can restore the entered name across sessions.
    #[serde(default)]
    display_name: Option<String>,
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

/// Get the account filename based on TUI mode
///
/// - Production mode: `account.json`
/// - Demo mode: `demo-account.json`
///
/// This ensures demo mode never overwrites a real account file.
fn account_filename(mode: TuiMode) -> &'static str {
    match mode {
        TuiMode::Production => "account.json",
        TuiMode::Demo { .. } => "demo-account.json",
    }
}

/// Get the journal filename based on TUI mode
///
/// - Production mode: `journal.json`
/// - Demo mode: `demo-journal.json`
///
/// This ensures demo mode never overwrites a real journal file.
fn journal_filename(mode: TuiMode) -> &'static str {
    match mode {
        TuiMode::Production => "journal.json",
        TuiMode::Demo { .. } => "demo-journal.json",
    }
}

/// Clean up demo files before starting demo mode
///
/// In demo mode, we want users to go through the account creation flow each time.
/// This function deletes demo-account.json and demo-journal.json if they exist.
///
/// This is ONLY called in demo mode - production files are NEVER deleted.
async fn cleanup_demo_files(storage: &impl StorageEffects) {
    for key in ["demo-account.json", "demo-journal.json"] {
        match storage.remove(key).await {
            Ok(true) => tracing::debug!(key = key, "Removed demo file"),
            Ok(false) => {}
            Err(e) => tracing::warn!(key = key, err = %e, "Failed to remove demo file"),
        }
    }
}

/// Try to load an existing account configuration
///
/// Returns `Loaded` if account exists, `NotFound` otherwise.
/// Does NOT auto-create accounts - this allows the TUI to show an account setup modal.
async fn try_load_account(
    storage: &impl StorageEffects,
    mode: TuiMode,
) -> Result<AccountLoadResult, AuraError> {
    let Some(bytes) = storage
        .retrieve(account_filename(mode))
        .await
        .map_err(|e| AuraError::internal(format!("Failed to read account config: {}", e)))?
    else {
        return Ok(AccountLoadResult::NotFound);
    };

    let content = String::from_utf8(bytes)
        .map_err(|e| AuraError::internal(format!("Invalid account config UTF-8: {}", e)))?;

    let config: AccountConfig = serde_json::from_str(&content)
        .map_err(|e| AuraError::internal(format!("Failed to parse account config: {}", e)))?;

    // Parse authority ID from hex string (16 bytes = UUID)
    let authority_bytes: [u8; 16] = hex::decode(&config.authority_id)
        .map_err(|e| AuraError::internal(format!("Invalid authority_id hex: {}", e)))?
        .try_into()
        .map_err(|_| AuraError::internal("Invalid authority_id length (expected 16 bytes)"))?;
    let authority_id = AuthorityId::from_uuid(uuid::Uuid::from_bytes(authority_bytes));

    // Parse context ID from hex string (16 bytes = UUID)
    let context_bytes: [u8; 16] = hex::decode(&config.context_id)
        .map_err(|e| AuraError::internal(format!("Invalid context_id hex: {}", e)))?
        .try_into()
        .map_err(|_| AuraError::internal("Invalid context_id length (expected 16 bytes)"))?;
    let context_id = ContextId::from_uuid(uuid::Uuid::from_bytes(context_bytes));

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
    // Use the same deterministic derivation as `create_account`.
    //
    // The "placeholder" status is tracked separately via `has_existing_account`.
    // Keeping the identity stable avoids needing to rebuild the runtime after account creation.
    let authority_entropy =
        aura_core::hash::hash(format!("authority:{}", device_id_str).as_bytes());
    let context_entropy = aura_core::hash::hash(format!("context:{}", device_id_str).as_bytes());

    (
        AuthorityId::new_from_entropy(authority_entropy),
        ContextId::new_from_entropy(context_entropy),
    )
}

async fn persist_account_config(
    storage: &impl StorageEffects,
    time: &impl PhysicalTimeEffects,
    mode: TuiMode,
    authority_id: AuthorityId,
    context_id: ContextId,
    display_name: Option<String>,
) -> Result<(), AuraError> {
    let created_at = time
        .physical_time()
        .await
        .map_err(|e| AuraError::internal(format!("Failed to fetch physical time: {}", e)))?
        .ts_ms;

    let config = AccountConfig {
        authority_id: hex::encode(authority_id.to_bytes()),
        context_id: hex::encode(context_id.to_bytes()),
        display_name,
        created_at,
    };

    let content = serde_json::to_string_pretty(&config)
        .map_err(|e| AuraError::internal(format!("Failed to serialize account config: {}", e)))?;

    storage
        .store(account_filename(mode), content.into_bytes())
        .await
        .map_err(|e| AuraError::internal(format!("Failed to write account config: {}", e)))?;

    Ok(())
}

/// Create a new account and save to disk
///
/// Called when user completes the account setup modal.
pub async fn create_account(
    base_path: &Path,
    device_id_str: &str,
    mode: TuiMode,
    display_name: &str,
) -> Result<(AuthorityId, ContextId), AuraError> {
    let storage = PathFilesystemStorageHandler::new(base_path.to_path_buf());
    let time = PhysicalTimeHandler::new();

    // Create new account with deterministic IDs based on device_id
    // This ensures the same device_id always creates the same account
    let authority_entropy =
        aura_core::hash::hash(format!("authority:{}", device_id_str).as_bytes());
    let context_entropy = aura_core::hash::hash(format!("context:{}", device_id_str).as_bytes());

    let authority_id = AuthorityId::new_from_entropy(authority_entropy);
    let context_id = ContextId::new_from_entropy(context_entropy);

    // Persist to storage using effect-backed handlers.
    persist_account_config(
        &storage,
        &time,
        mode,
        authority_id,
        context_id,
        Some(display_name.to_string()),
    )
    .await?;

    Ok((authority_id, context_id))
}

/// Restore an account from guardian-based recovery
///
/// This is used after catastrophic device loss where guardians have
/// reconstructed the ORIGINAL authority_id via FROST threshold signatures.
/// Unlike `create_account()` which derives from device_id, this preserves
/// the cryptographically identical authority from before the loss.
///
/// # Arguments
/// * `base_path` - Data directory for account storage
/// * `recovered_authority_id` - The ORIGINAL authority_id reconstructed by guardians
/// * `recovered_context_id` - Optional context_id (generated deterministically if None)
///
/// # Returns
/// * The authority and context IDs written to disk
pub async fn restore_recovered_account(
    base_path: &Path,
    recovered_authority_id: AuthorityId,
    recovered_context_id: Option<ContextId>,
    mode: TuiMode,
) -> Result<(AuthorityId, ContextId), AuraError> {
    let storage = PathFilesystemStorageHandler::new(base_path.to_path_buf());
    let time = PhysicalTimeHandler::new();

    // Use the recovered authority directly (NOT derived from device_id)
    // This is the key difference from create_account()
    let authority_bytes = recovered_authority_id.to_bytes();

    // For context, either use the recovered one or derive deterministically from authority
    let context_id = recovered_context_id.unwrap_or_else(|| {
        let context_entropy = aura_core::hash::hash(
            format!("context:recovered:{}", hex::encode(authority_bytes)).as_bytes(),
        );
        ContextId::new_from_entropy(context_entropy)
    });

    persist_account_config(
        &storage,
        &time,
        mode,
        recovered_authority_id,
        context_id,
        None,
    )
    .await?;

    Ok((recovered_authority_id, context_id))
}

// =============================================================================
// Account Backup/Export
// =============================================================================

/// Current backup format version
const BACKUP_VERSION: u32 = 1;

/// Backup format prefix for identification
const BACKUP_PREFIX: &str = "aura:backup:v1:";

/// Complete account backup data structure
///
/// Contains all data needed to restore an account on a new device:
/// - Account configuration (authority_id, context_id, created_at)
/// - Journal facts (all state history)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountBackup {
    /// Backup format version
    pub version: u32,
    /// Account configuration
    pub account: AccountConfig,
    /// Journal content (JSON string of all facts)
    pub journal: Option<String>,
    /// Backup creation timestamp (ms since epoch)
    pub backup_at: u64,
    /// Device ID that created the backup (informational only)
    pub source_device: Option<String>,
}

/// Export account to a portable backup code
///
/// The backup code is a base64-encoded JSON blob with a prefix for easy identification.
/// Format: `aura:backup:v1:<base64>`
///
/// # Arguments
/// * `base_path` - Data directory containing account and journal files
/// * `device_id` - Optional device ID to include in backup metadata
/// * `mode` - TUI mode (determines which account/journal files to export)
///
/// # Returns
/// * Portable backup code string
pub async fn export_account_backup(
    base_path: &Path,
    device_id: Option<&str>,
    mode: TuiMode,
) -> Result<String, AuraError> {
    let storage = PathFilesystemStorageHandler::new(base_path.to_path_buf());
    let time = PhysicalTimeHandler::new();

    let Some(account_bytes) = storage
        .retrieve(account_filename(mode))
        .await
        .map_err(|e| AuraError::internal(format!("Failed to read account config: {}", e)))?
    else {
        return Err(AuraError::internal("No account exists to backup"));
    };

    let account_content = String::from_utf8(account_bytes)
        .map_err(|e| AuraError::internal(format!("Invalid account config UTF-8: {}", e)))?;

    let account: AccountConfig = serde_json::from_str(&account_content)
        .map_err(|e| AuraError::internal(format!("Failed to parse account config: {}", e)))?;

    let journal = storage
        .retrieve(journal_filename(mode))
        .await
        .map_err(|e| AuraError::internal(format!("Failed to read journal: {}", e)))?
        .and_then(|b| String::from_utf8(b).ok());

    let backup_at = time
        .physical_time()
        .await
        .map_err(|e| AuraError::internal(format!("Failed to fetch physical time: {}", e)))?
        .ts_ms;

    // Create backup structure
    let backup = AccountBackup {
        version: BACKUP_VERSION,
        account,
        journal,
        backup_at,
        source_device: device_id.map(String::from),
    };

    // Serialize to JSON
    let json = serde_json::to_string(&backup)
        .map_err(|e| AuraError::internal(format!("Failed to serialize backup: {}", e)))?;

    // Encode as base64 with prefix
    use base64::Engine;
    let encoded = base64::engine::general_purpose::STANDARD.encode(json.as_bytes());

    Ok(format!("{}{}", BACKUP_PREFIX, encoded))
}

/// Import and restore account from backup code
///
/// # Arguments
/// * `base_path` - Data directory to restore account to
/// * `backup_code` - The backup code from `export_account_backup`
/// * `overwrite` - If true, overwrite existing account; if false, fail if account exists
/// * `mode` - TUI mode (determines which account file to import to)
///
/// # Returns
/// * The restored authority and context IDs
pub async fn import_account_backup(
    base_path: &Path,
    backup_code: &str,
    overwrite: bool,
    mode: TuiMode,
) -> Result<(AuthorityId, ContextId), AuraError> {
    let storage = PathFilesystemStorageHandler::new(base_path.to_path_buf());

    // Parse backup code
    if !backup_code.starts_with(BACKUP_PREFIX) {
        return Err(AuraError::internal(format!(
            "Invalid backup code format (expected prefix '{}')",
            BACKUP_PREFIX
        )));
    }

    let encoded = &backup_code[BACKUP_PREFIX.len()..];

    // Decode base64
    use base64::Engine;
    let json_bytes = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .map_err(|e| AuraError::internal(format!("Invalid backup code encoding: {}", e)))?;

    let json = String::from_utf8(json_bytes)
        .map_err(|e| AuraError::internal(format!("Invalid backup code UTF-8: {}", e)))?;

    // Parse backup structure
    let backup: AccountBackup = serde_json::from_str(&json)
        .map_err(|e| AuraError::internal(format!("Invalid backup format: {}", e)))?;

    // Validate version
    if backup.version > BACKUP_VERSION {
        return Err(AuraError::internal(format!(
            "Backup version {} is newer than supported version {}",
            backup.version, BACKUP_VERSION
        )));
    }

    // Parse authority ID
    let authority_bytes: [u8; 16] = hex::decode(&backup.account.authority_id)
        .map_err(|e| AuraError::internal(format!("Invalid authority_id in backup: {}", e)))?
        .try_into()
        .map_err(|_| AuraError::internal("Invalid authority_id length in backup"))?;
    let authority_id = AuthorityId::from_uuid(uuid::Uuid::from_bytes(authority_bytes));

    // Parse context ID
    let context_bytes: [u8; 16] = hex::decode(&backup.account.context_id)
        .map_err(|e| AuraError::internal(format!("Invalid context_id in backup: {}", e)))?
        .try_into()
        .map_err(|_| AuraError::internal("Invalid context_id length in backup"))?;
    let context_id = ContextId::from_uuid(uuid::Uuid::from_bytes(context_bytes));

    // Check for existing account
    if storage
        .exists(account_filename(mode))
        .await
        .map_err(|e| AuraError::internal(format!("Failed to check account existence: {}", e)))?
        && !overwrite
    {
        return Err(AuraError::internal(
            "Account already exists. Use overwrite=true to replace.",
        ));
    }

    // Write account configuration (pretty JSON for readability).
    let account_content = serde_json::to_string_pretty(&backup.account)
        .map_err(|e| AuraError::internal(format!("Failed to serialize account config: {}", e)))?;

    storage
        .store(account_filename(mode), account_content.into_bytes())
        .await
        .map_err(|e| AuraError::internal(format!("Failed to write account config: {}", e)))?;

    // Write journal if present in backup
    if let Some(ref journal_content) = backup.journal {
        storage
            .store(journal_filename(mode), journal_content.as_bytes().to_vec())
            .await
            .map_err(|e| AuraError::internal(format!("Failed to write journal: {}", e)))?;
    }

    Ok((authority_id, context_id))
}

/// Demo seed for deterministic simulation
const DEMO_SEED: u64 = 2024;

/// Handle TUI launch
pub async fn handle_tui(args: &TuiArgs) -> crate::error::TerminalResult<()> {
    let stdio = PreFullscreenStdio::new();

    // Demo mode: use simulation backend with deterministic seed
    // The TUI code is IDENTICAL for demo and production - only the backend differs
    if args.demo {
        stdio.println(format_args!("Starting Aura TUI (Demo Mode)"));
        stdio.println(format_args!("============================="));
        stdio.println(format_args!(
            "Demo mode runs a real agent with simulated effects."
        ));
        stdio.println(format_args!("Seed: {} (deterministic)", DEMO_SEED));
        stdio.newline();

        // Use demo-specific data directory and device ID
        let demo_data_dir = args
            .data_dir
            .clone()
            .unwrap_or_else(|| "./aura-data".to_string());
        let demo_device_id = args
            .device_id
            .clone()
            .unwrap_or_else(|| "demo:bob".to_string());

        return handle_tui_launch(
            stdio,
            Some(&demo_data_dir),
            Some(&demo_device_id),
            TuiMode::Demo { seed: DEMO_SEED },
        )
        .await;
    }

    let data_dir = args.data_dir.clone().or_else(|| env::var("AURA_PATH").ok());
    handle_tui_launch(
        stdio,
        data_dir.as_deref(),
        args.device_id.as_deref(),
        TuiMode::Production,
    )
    .await
}

/// Launch the TUI with the specified mode
async fn handle_tui_launch(
    stdio: PreFullscreenStdio,
    data_dir: Option<&str>,
    device_id_str: Option<&str>,
    mode: TuiMode,
) -> crate::error::TerminalResult<()> {
    let mode_str = match mode {
        TuiMode::Production => "Production",
        TuiMode::Demo { .. } => "Demo (Simulation)",
    };
    stdio.println(format_args!("Starting Aura TUI ({})", mode_str));
    stdio.println(format_args!("================"));

    // Determine data directory
    let base_path = data_dir
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("./aura-data"));

    // Initialize tracing for TUI into a file (avoid stderr corruption in fullscreen).
    // Safe to call multiple times; only the first init wins.
    init_tui_tracing(&base_path, mode);

    let storage = PathFilesystemStorageHandler::new(base_path.clone());

    // In demo mode, clean up existing demo files so users go through account creation
    // This is ONLY done in demo mode - production files are NEVER deleted
    if matches!(mode, TuiMode::Demo { .. }) {
        cleanup_demo_files(&storage).await;
    }

    // Determine device ID
    let device_id = device_id_str
        .map(|id| crate::ids::device_id(id))
        .unwrap_or_else(|| crate::ids::device_id("tui:production-device"));

    stdio.println(format_args!("Data directory: {}", base_path.display()));
    stdio.println(format_args!("Device ID: {}", device_id));

    // Determine device ID string for account derivation
    let device_id_for_account = device_id_str.unwrap_or("tui:production-device");

    // Try to load existing account, or use placeholders if no account exists
    let (authority_id, context_id, has_existing_account) =
        match try_load_account(&storage, mode).await? {
            AccountLoadResult::Loaded { authority, context } => {
                stdio.println(format_args!("Authority: {}", authority));
                stdio.println(format_args!("Context: {}", context));
                (authority, context, true)
            }
            AccountLoadResult::NotFound => {
                // Use placeholder IDs - the TUI will show account setup modal
                let (authority, context) = create_placeholder_ids(device_id_for_account);
                stdio.println(format_args!("No existing account - will show setup modal"));
                stdio.println(format_args!("Placeholder Authority: {}", authority));
                stdio.println(format_args!("Placeholder Context: {}", context));
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

    // In demo mode, create simulator first to get shared transport inbox
    #[cfg(feature = "development")]
    let demo_simulator_for_bob = match mode {
        TuiMode::Demo { seed } => {
            stdio.println(format_args!(
                "Creating demo simulator for shared transport..."
            ));
            let sim = DemoSimulator::new(seed)
                .await
                .map_err(|e| AuraError::internal(format!("Failed to create simulator: {}", e)))?;
            Some(sim)
        }
        TuiMode::Production => None,
    };

    #[cfg(not(feature = "development"))]
    let _demo_simulator_for_bob: Option<()> = None;

    // Build agent using appropriate builder method based on mode
    let agent = match mode {
        TuiMode::Production => AgentBuilder::new()
            .with_config(agent_config)
            .with_authority(authority_id)
            .build_production(&effect_ctx)
            .await
            .map_err(|e| AuraError::internal(format!("Failed to create agent: {}", e)))?,
        TuiMode::Demo { seed } => {
            stdio.println(format_args!("Using simulation agent with seed: {}", seed));

            #[cfg(feature = "development")]
            {
                // Use shared transport inbox from simulator
                let shared_inbox = demo_simulator_for_bob
                    .as_ref()
                    .map(|sim| sim.shared_transport_inbox.clone())
                    .expect("Simulator should be created in demo mode");

                stdio.println(format_args!(
                    "Creating Bob's agent with shared transport..."
                ));
                AgentBuilder::new()
                    .with_config(agent_config)
                    .with_authority(authority_id)
                    .build_simulation_async_with_shared_transport(seed, &effect_ctx, shared_inbox)
                    .await
                    .map_err(|e| {
                        AuraError::internal(format!(
                            "Failed to create simulation agent with shared transport: {}",
                            e
                        ))
                    })?
            }

            #[cfg(not(feature = "development"))]
            {
                // Fallback for non-development builds
                AgentBuilder::new()
                    .with_config(agent_config)
                    .with_authority(authority_id)
                    .build_simulation_async(seed, &effect_ctx)
                    .await
                    .map_err(|e| {
                        AuraError::internal(format!("Failed to create simulation agent: {}", e))
                    })?
            }
        }
    };

    // Create AppCore with runtime bridge - the portable application core from aura-app.
    // The runtime is responsible for committing facts and driving reactive signals.
    let app_config = AppConfig {
        data_dir: base_path.to_string_lossy().to_string(),
        debug: false,
        journal_path: None,
    };
    let agent = Arc::new(agent);
    let app_core = AppCore::with_runtime(app_config, agent.clone().as_runtime_bridge())
        .map_err(|e| AuraError::internal(format!("Failed to create AppCore: {}", e)))?;

    let app_core = Arc::new(RwLock::new(app_core));

    let app_core = InitializedAppCore::new(app_core).await?;

    if let Err(e) =
        aura_app::workflows::settings::refresh_settings_from_runtime(app_core.raw()).await
    {
        stdio.eprintln(format_args!("Warning: Failed to refresh settings: {}", e));
    }

    stdio.println(format_args!(
        "AppCore initialized (with runtime bridge and reactive signals)"
    ));

    // In demo mode, start the simulator with Alice and Carol peer agents
    // DemoSignalCoordinator handles bidirectional event routing via signals
    #[cfg(feature = "development")]
    let (mut simulator, _coordinator_handles): (
        Option<DemoSimulator>,
        Option<(tokio::task::JoinHandle<()>, tokio::task::JoinHandle<()>)>,
    ) = match mode {
        TuiMode::Demo { seed: _ } => {
            // Use the simulator we already created for shared transport
            stdio.println(format_args!("Starting demo simulator..."));
            let mut sim = demo_simulator_for_bob.expect("Simulator should exist in demo mode");

            sim.start()
                .await
                .map_err(|e| AuraError::internal(format!("Failed to start simulator: {}", e)))?;

            let alice_id = sim.alice_authority().await;
            let carol_id = sim.carol_authority().await;
            stdio.println(format_args!("Alice online: {}", alice_id));
            stdio.println(format_args!("Carol online: {}", carol_id));
            stdio.println(format_args!("Peers connected: {}", sim.peer_count()));

            // Get the simulator's bridge and response receiver for signal coordinator
            let sim_bridge = sim.bridge();
            let response_rx = sim
                .take_response_receiver()
                .await
                .ok_or_else(|| AuraError::internal("Response receiver already taken"))?;

            // Create DemoSignalCoordinator - handles bidirectional event routing via signals
            // This replaces the manual AuraEvent forwarding with signal subscriptions
            let coordinator = Arc::new(DemoSignalCoordinator::new(
                app_core.raw().clone(),
                authority_id, // Bob's authority
                sim_bridge,
                response_rx,
            ));

            // Start coordinator tasks
            let handles = coordinator.start();
            stdio.println(format_args!("Demo signal coordinator started"));

            // Start background task to process ceremony acceptances
            let ceremony_agent = agent.clone();
            tokio::spawn(async move {
                let mut interval = tokio::time::interval(tokio::time::Duration::from_millis(500));
                loop {
                    interval.tick().await;
                    match ceremony_agent.process_ceremony_acceptances().await {
                        Ok((acceptances, completions)) => {
                            if acceptances > 0 {
                                tracing::info!(
                                    acceptances = acceptances,
                                    completions = completions,
                                    "Processed guardian ceremony acceptances"
                                );
                            }
                        }
                        Err(e) => {
                            tracing::warn!("Error processing ceremony acceptances: {}", e);
                        }
                    }
                }
            });
            stdio.println(format_args!("Ceremony acceptance processor started"));

            (Some(sim), Some(handles))
        }
        TuiMode::Production => (None, None),
    };

    // Create IoContext with AppCore integration
    // Pass has_existing_account so the TUI knows whether to show the account setup modal
    // Also pass base_path and device_id for account file creation
    // In demo mode, include hints with Alice/Carol invite codes AND the demo bridge
    #[cfg(feature = "development")]
    let ctx = match mode {
        TuiMode::Demo { seed } => {
            let hints = crate::demo::DemoHints::new(seed);
            stdio.println(format_args!("Demo hints available:"));
            stdio.println(format_args!(
                "  Alice invite code: {}",
                hints.alice_invite_code
            ));
            stdio.println(format_args!(
                "  Carol invite code: {}",
                hints.carol_invite_code
            ));
            let mut builder = IoContext::builder()
                .with_app_core(app_core)
                .with_base_path(base_path.clone())
                .with_device_id(device_id_for_account.to_string())
                .with_mode(mode)
                .with_existing_account(has_existing_account)
                .with_demo_hints(hints);

            // Wire up the demo bridge so IoContext.dispatch() routes commands to simulated agents
            // This allows Alice/Carol to respond to guardian invitations and other interactions
            if let Some(ref sim) = simulator {
                builder = builder.with_demo_bridge(sim.bridge());
                stdio.println(format_args!("Demo bridge connected to IoContext"));
            }

            builder
                .build()
                .expect("IoContext build failed with all required fields")
        }
        TuiMode::Production => IoContext::builder()
            .with_app_core(app_core.clone())
            .with_base_path(base_path.clone())
            .with_device_id(device_id_for_account.to_string())
            .with_mode(mode)
            .with_existing_account(has_existing_account)
            .build()
            .expect("IoContext build failed with all required fields"),
    };

    #[cfg(not(feature = "development"))]
    let ctx = IoContext::builder()
        .with_app_core(app_core.clone())
        .with_base_path(base_path.clone())
        .with_device_id(device_id_for_account.to_string())
        .with_mode(mode)
        .with_existing_account(has_existing_account)
        .build()
        .expect("IoContext build failed with all required fields");

    // Without development feature, demo mode just shows a warning
    #[cfg(not(feature = "development"))]
    if matches!(mode, TuiMode::Demo { .. }) {
        stdio.println(format_args!(
            "Note: Demo mode simulation requires the 'development' feature."
        ));
        stdio.println(format_args!(
            "Running with simulation agent but without peer agents (Alice/Carol)."
        ));
    }

    stdio.println(format_args!("Launching TUI..."));
    stdio.newline();

    // Run the iocraft TUI app with context
    let (_stdio, result) = during_fullscreen(stdio, run_app_with_context(ctx)).await;
    let result = result.map_err(|e| AuraError::internal(format!("TUI failed: {}", e)));

    // In demo mode, stop the simulator cleanly
    #[cfg(feature = "development")]
    if let Some(ref mut sim) = simulator {
        _stdio.println(format_args!("Stopping demo simulator..."));
        if let Err(e) = sim.stop().await {
            _stdio.eprintln(format_args!(
                "Warning: Failed to stop simulator cleanly: {}",
                e
            ));
        }
    }

    result?;
    Ok(())
}

#[allow(clippy::expect_used)] // Panicking is appropriate when /dev/null can't be opened
fn init_tui_tracing(base_path: &Path, mode: TuiMode) {
    // Allow forcing stdio tracing for debugging.
    if std::env::var("AURA_TUI_ALLOW_STDIO").ok().as_deref() == Some("1") {
        return;
    }

    let default_name = match mode {
        TuiMode::Production => "aura-tui.log",
        TuiMode::Demo { .. } => "aura-tui-demo.log",
    };

    let log_path = std::env::var_os("AURA_TUI_LOG_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|| base_path.join(default_name));

    if let Some(parent) = log_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .unwrap_or_else(|_| {
            #[cfg(unix)]
            {
                std::fs::OpenOptions::new()
                    .write(true)
                    .open("/dev/null")
                    .expect("Failed to open /dev/null")
            }
            #[cfg(not(unix))]
            {
                std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&log_path)
                    .expect("Failed to open TUI log file")
            }
        });
    let file = std::sync::Arc::new(file);
    #[cfg(not(unix))]
    let log_path = std::sync::Arc::new(log_path);
    let make_writer = {
        let file = file.clone();
        #[cfg(not(unix))]
        let log_path = log_path.clone();
        move || {
            file.try_clone().unwrap_or_else(|_| {
                #[cfg(unix)]
                {
                    std::fs::OpenOptions::new()
                        .write(true)
                        .open("/dev/null")
                        .expect("Failed to open /dev/null")
                }
                #[cfg(not(unix))]
                {
                    std::fs::OpenOptions::new()
                        .create(true)
                        .append(true)
                        .open(log_path.as_ref())
                        .expect("Failed to open TUI log file")
                }
            })
        }
    };

    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_ansi(false)
        .with_target(false)
        .with_writer(make_writer)
        .try_init();
}
