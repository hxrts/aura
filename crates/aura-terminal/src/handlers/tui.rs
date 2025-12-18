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

use serde::{Deserialize, Serialize};
// Import app types from aura-app (pure layer)
use aura_app::{AppConfig, AppCore};
// Import agent types from aura-agent (runtime layer)
use async_lock::RwLock;
use aura_agent::core::config::StorageConfig;
use aura_agent::{AgentBuilder, AgentConfig, EffectContext};
use aura_core::identifiers::{AuthorityId, ContextId};
use aura_core::AuraError;

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
pub struct AccountConfig {
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
fn cleanup_demo_files(base_path: &Path) {
    let demo_account = base_path.join("demo-account.json");
    let demo_journal = base_path.join("demo-journal.json");

    if demo_account.exists() {
        if let Err(e) = std::fs::remove_file(&demo_account) {
            eprintln!(
                "Warning: Failed to remove demo account file: {} - {}",
                demo_account.display(),
                e
            );
        } else {
            println!("Removed existing demo account file");
        }
    }

    if demo_journal.exists() {
        if let Err(e) = std::fs::remove_file(&demo_journal) {
            eprintln!(
                "Warning: Failed to remove demo journal file: {} - {}",
                demo_journal.display(),
                e
            );
        } else {
            println!("Removed existing demo journal file");
        }
    }
}

/// Try to load an existing account configuration
///
/// Returns `Loaded` if account exists, `NotFound` otherwise.
/// Does NOT auto-create accounts - this allows the TUI to show an account setup modal.
fn try_load_account(base_path: &Path, mode: TuiMode) -> Result<AccountLoadResult, AuraError> {
    let account_path = base_path.join(account_filename(mode));

    if !account_path.exists() {
        return Ok(AccountLoadResult::NotFound);
    }

    // Load existing account
    let content = std::fs::read_to_string(&account_path)
        .map_err(|e| AuraError::internal(format!("Failed to read account config: {}", e)))?;

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
    mode: TuiMode,
) -> Result<(AuthorityId, ContextId), AuraError> {
    let account_path = base_path.join(account_filename(mode));

    // Create new account with deterministic IDs based on device_id
    // This ensures the same device_id always creates the same account
    let authority_entropy =
        aura_core::hash::hash(format!("authority:{}", device_id_str).as_bytes());
    let context_entropy = aura_core::hash::hash(format!("context:{}", device_id_str).as_bytes());

    let authority_id = AuthorityId::new_from_entropy(authority_entropy);
    let context_id = ContextId::new_from_entropy(context_entropy);

    // Save the new account configuration
    // Note: We store only the UUID bytes (16 bytes), not the full entropy (32 bytes)
    // since AuthorityId/ContextId only use the first 16 bytes of entropy anyway
    let config = AccountConfig {
        authority_id: hex::encode(authority_id.to_bytes()),
        context_id: hex::encode(context_id.to_bytes()),
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
pub fn restore_recovered_account(
    base_path: &Path,
    recovered_authority_id: AuthorityId,
    recovered_context_id: Option<ContextId>,
    mode: TuiMode,
) -> Result<(AuthorityId, ContextId), AuraError> {
    let account_path = base_path.join(account_filename(mode));

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
    let context_bytes = context_id.to_bytes();

    // Save the recovered account configuration
    let config = AccountConfig {
        authority_id: hex::encode(authority_bytes),
        context_id: hex::encode(context_bytes),
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
pub fn export_account_backup(
    base_path: &Path,
    device_id: Option<&str>,
    mode: TuiMode,
) -> Result<String, AuraError> {
    let account_path = base_path.join(account_filename(mode));
    let journal_path = base_path.join(journal_filename(mode));

    // Load account configuration
    if !account_path.exists() {
        return Err(AuraError::internal("No account exists to backup"));
    }

    let account_content = std::fs::read_to_string(&account_path)
        .map_err(|e| AuraError::internal(format!("Failed to read account config: {}", e)))?;

    let account: AccountConfig = serde_json::from_str(&account_content)
        .map_err(|e| AuraError::internal(format!("Failed to parse account config: {}", e)))?;

    // Load journal if it exists
    let journal = if journal_path.exists() {
        std::fs::read_to_string(&journal_path).ok()
    } else {
        None
    };

    // Create backup structure
    let backup = AccountBackup {
        version: BACKUP_VERSION,
        account,
        journal,
        backup_at: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0),
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
pub fn import_account_backup(
    base_path: &Path,
    backup_code: &str,
    overwrite: bool,
    mode: TuiMode,
) -> Result<(AuthorityId, ContextId), AuraError> {
    let account_path = base_path.join(account_filename(mode));
    let journal_path = base_path.join(journal_filename(mode));

    // Check for existing account
    if account_path.exists() && !overwrite {
        return Err(AuraError::internal(
            "Account already exists. Use overwrite=true to replace.",
        ));
    }

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

    // Ensure directory exists
    if let Some(parent) = account_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| AuraError::internal(format!("Failed to create data directory: {}", e)))?;
    }

    // Write account configuration
    let account_content = serde_json::to_string_pretty(&backup.account)
        .map_err(|e| AuraError::internal(format!("Failed to serialize account config: {}", e)))?;

    std::fs::write(&account_path, account_content)
        .map_err(|e| AuraError::internal(format!("Failed to write account config: {}", e)))?;

    // Write journal if present in backup
    if let Some(ref journal_content) = backup.journal {
        std::fs::write(&journal_path, journal_content)
            .map_err(|e| AuraError::internal(format!("Failed to write journal: {}", e)))?;
    }

    Ok((authority_id, context_id))
}

/// Demo seed for deterministic simulation
const DEMO_SEED: u64 = 2024;

/// Handle TUI launch
pub async fn handle_tui(args: &TuiArgs) -> crate::error::TerminalResult<()> {
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
            .unwrap_or_else(|| "./aura-data".to_string());
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
) -> crate::error::TerminalResult<()> {
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

    // In demo mode, clean up existing demo files so users go through account creation
    // This is ONLY done in demo mode - production files are NEVER deleted
    if matches!(mode, TuiMode::Demo { .. }) {
        // Ensure the data directory exists before cleanup
        if let Err(e) = std::fs::create_dir_all(&base_path) {
            eprintln!("Warning: Failed to create data directory: {}", e);
        }
        cleanup_demo_files(&base_path);
    }

    // Determine device ID
    let device_id = device_id_str
        .map(|id| crate::ids::device_id(id))
        .unwrap_or_else(|| crate::ids::device_id("tui:production-device"));

    println!("Data directory: {}", base_path.display());
    println!("Device ID: {}", device_id);

    // Determine device ID string for account derivation
    let device_id_for_account = device_id_str.unwrap_or("tui:production-device");

    // Try to load existing account, or use placeholders if no account exists
    let (authority_id, context_id, has_existing_account) = match try_load_account(&base_path, mode)?
    {
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

    // In demo mode, create simulator first to get shared transport inbox
    #[cfg(feature = "development")]
    let demo_simulator_for_bob = match mode {
        TuiMode::Demo { seed } => {
            println!("Creating demo simulator for shared transport...");
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
            println!("Using simulation agent with seed: {}", seed);

            #[cfg(feature = "development")]
            {
                // Use shared transport inbox from simulator
                let shared_inbox = demo_simulator_for_bob
                    .as_ref()
                    .map(|sim| sim.shared_transport_inbox.clone())
                    .expect("Simulator should be created in demo mode");

                println!("Creating Bob's agent with shared transport...");
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

    // Create AppCore with runtime bridge - the portable application core from aura-app
    // This provides intent-based state management with reactive ViewState
    // The agent implements RuntimeBridge, enabling the dependency inversion
    let journal_path = base_path.join(journal_filename(mode));
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

    // In demo mode, start the simulator with Alice and Carol peer agents
    // DemoSignalCoordinator handles bidirectional event routing via signals
    #[cfg(feature = "development")]
    let (mut simulator, _coordinator_handles): (
        Option<DemoSimulator>,
        Option<(tokio::task::JoinHandle<()>, tokio::task::JoinHandle<()>)>,
    ) = match mode {
        TuiMode::Demo { seed: _ } => {
            // Use the simulator we already created for shared transport
            println!("Starting demo simulator...");
            let mut sim = demo_simulator_for_bob.expect("Simulator should exist in demo mode");

            sim.start()
                .await
                .map_err(|e| AuraError::internal(format!("Failed to start simulator: {}", e)))?;

            let alice_id = sim.alice_authority().await;
            let carol_id = sim.carol_authority().await;
            println!("Alice online: {}", alice_id);
            println!("Carol online: {}", carol_id);
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
            println!("Ceremony acceptance processor started");

            (Some(sim), Some(handles))
        }
        TuiMode::Production => (None, None),
    };

    // Create IoContext with AppCore integration
    // Pass has_existing_account so the TUI knows whether to show the account setup modal
    // Also pass base_path and device_id for account file creation
    // In demo mode, include hints with Alice/Carol invite codes
    #[cfg(feature = "development")]
    let ctx = match mode {
        TuiMode::Demo { seed } => {
            let hints = crate::demo::DemoHints::new(seed);
            println!("Demo hints available:");
            println!("  Alice invite code: {}", hints.alice_invite_code);
            println!("  Carol invite code: {}", hints.carol_invite_code);
            IoContext::with_demo_hints(
                app_core,
                hints,
                has_existing_account,
                base_path.clone(),
                device_id_for_account.to_string(),
                mode,
            )
        }
        TuiMode::Production => IoContext::with_account_status(
            app_core.clone(),
            has_existing_account,
            base_path.clone(),
            device_id_for_account.to_string(),
            mode,
        ),
    };

    #[cfg(not(feature = "development"))]
    let ctx = IoContext::with_account_status(
        app_core.clone(),
        has_existing_account,
        base_path.clone(),
        device_id_for_account.to_string(),
        mode,
    );

    // Without development feature, demo mode just shows a warning
    #[cfg(not(feature = "development"))]
    if matches!(mode, TuiMode::Demo { .. }) {
        println!("Note: Demo mode simulation requires the 'development' feature.");
        println!("Running with simulation agent but without peer agents (Alice/Carol).");
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
