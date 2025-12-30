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
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
// Import app types from aura-app (pure layer)
use aura_app::ui::prelude::*;
// Import agent types from aura-agent (runtime layer)
use async_lock::RwLock;
use aura_agent::core::config::StorageConfig;
use aura_agent::{AgentBuilder, AgentConfig, EffectContext};
use aura_core::effects::time::PhysicalTimeEffects;
use aura_core::effects::{StorageCoreEffects, StorageExtendedEffects};
use aura_core::identifiers::{AuthorityId, ContextId};
use aura_core::AuraError;
use aura_effects::time::PhysicalTimeHandler;
use aura_effects::{
    EncryptedStorage, EncryptedStorageConfig, FilesystemStorageHandler, RealCryptoHandler,
    RealSecureStorageHandler,
};
use tokio::sync::mpsc;
use tracing_subscriber::EnvFilter;

use crate::cli::tui::TuiArgs;
#[cfg(feature = "development")]
use crate::demo::DemoSimulator;
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

/// Account configuration filename
///
/// Mode isolation is achieved through separate base directories:
/// - Production: `$AURA_PATH/.aura/account.json` (default: `~/.aura/account.json`)
/// - Demo: `$AURA_PATH/.aura-demo/account.json` (default: `~/.aura-demo/account.json`)
const ACCOUNT_FILENAME: &str = "account.json";

/// Journal filename
///
/// Mode isolation is achieved through separate base directories:
/// - Production: `$AURA_PATH/.aura/journal.json` (default: `~/.aura/journal.json`)
/// - Demo: `$AURA_PATH/.aura-demo/journal.json` (default: `~/.aura-demo/journal.json`)
const JOURNAL_FILENAME: &str = "journal.json";
const TUI_LOG_KEY_PREFIX: &str = "logs";
const MAX_TUI_LOG_BYTES: usize = 1_000_000;
const TUI_LOG_QUEUE_CAPACITY: usize = 256;

type BootstrapStorage =
    EncryptedStorage<FilesystemStorageHandler, RealCryptoHandler, RealSecureStorageHandler>;

fn open_bootstrap_storage(base_path: &Path) -> BootstrapStorage {
    let crypto = Arc::new(RealCryptoHandler::new());
    let secure = Arc::new(RealSecureStorageHandler::with_base_path(
        base_path.to_path_buf(),
    ));
    EncryptedStorage::new(
        FilesystemStorageHandler::from_path(base_path.to_path_buf()),
        crypto,
        secure,
        EncryptedStorageConfig::default(),
    )
}

/// Resolve the storage base path for Aura.
///
/// This is the SINGLE SOURCE OF TRUTH for storage path resolution.
///
/// # Priority (highest to lowest):
/// 1. Explicit override (from --data-dir flag) - uses the path as-is
/// 2. $AURA_PATH environment variable + mode suffix
/// 3. Home directory (~) + mode suffix
///
/// # Mode Suffixes:
/// - Production: `.aura`
/// - Demo: `.aura-demo`
///
/// # Examples:
/// - Production with no override: `~/.aura`
/// - Demo with $AURA_PATH=/project: `/project/.aura-demo`
/// - Explicit override `./my-data`: uses `./my-data` exactly
pub fn resolve_storage_path(explicit_override: Option<&str>, mode: TuiMode) -> PathBuf {
    // If explicit override provided, use it directly (user knows what they want)
    if let Some(path) = explicit_override {
        return PathBuf::from(path);
    }

    // Determine base from $AURA_PATH or home directory
    let aura_path = env::var("AURA_PATH")
        .ok()
        .map(PathBuf::from)
        .or_else(dirs::home_dir)
        .unwrap_or_else(|| PathBuf::from("."));

    // Append mode-specific suffix
    match mode {
        TuiMode::Production => aura_path.join(".aura"),
        TuiMode::Demo { .. } => aura_path.join(".aura-demo"),
    }
}

/// Clean up demo directory before starting demo mode
///
/// Demo mode uses a dedicated directory (`.aura-demo`) that is completely
/// separate from production data (`.aura`). On startup, we delete the entire
/// directory to ensure a clean slate for each demo session.
///
/// This is ONLY called in demo mode - production directory is NEVER touched.
async fn cleanup_demo_storage(storage: &impl StorageExtendedEffects, base_path: &Path) {
    match storage.clear_all().await {
        Ok(()) => tracing::info!(path = %base_path.display(), "Cleaned up demo storage"),
        Err(e) => tracing::warn!(
            path = %base_path.display(),
            err = %e,
            "Failed to clean up demo storage"
        ),
    }
}

/// Try to load an existing account configuration
///
/// Returns `Loaded` if account exists, `NotFound` otherwise.
/// Does NOT auto-create accounts - this allows the TUI to show an account setup modal.
async fn try_load_account(
    storage: &impl StorageCoreEffects,
) -> Result<AccountLoadResult, AuraError> {
    let Some(bytes) = storage
        .retrieve(ACCOUNT_FILENAME)
        .await
        .map_err(|e| AuraError::internal(format!("Failed to read account config: {e}")))?
    else {
        return Ok(AccountLoadResult::NotFound);
    };

    let config: AccountConfig = serde_json::from_slice(&bytes)
        .map_err(|e| AuraError::internal(format!("Failed to parse account config: {e}")))?;

    // Parse authority ID from hex string (16 bytes = UUID)
    let authority_bytes: [u8; 16] = hex::decode(&config.authority_id)
        .map_err(|e| AuraError::internal(format!("Invalid authority_id hex: {e}")))?
        .try_into()
        .map_err(|_| AuraError::internal("Invalid authority_id length (expected 16 bytes)"))?;
    let authority_id = AuthorityId::from_uuid(uuid::Uuid::from_bytes(authority_bytes));

    // Parse context ID from hex string (16 bytes = UUID)
    let context_bytes: [u8; 16] = hex::decode(&config.context_id)
        .map_err(|e| AuraError::internal(format!("Invalid context_id hex: {e}")))?
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
    let authority_entropy = aura_core::hash::hash(format!("authority:{device_id_str}").as_bytes());
    let context_entropy = aura_core::hash::hash(format!("context:{device_id_str}").as_bytes());

    (
        AuthorityId::new_from_entropy(authority_entropy),
        ContextId::new_from_entropy(context_entropy),
    )
}

async fn persist_account_config(
    storage: &impl StorageCoreEffects,
    time: &impl PhysicalTimeEffects,
    authority_id: AuthorityId,
    context_id: ContextId,
    display_name: Option<String>,
) -> Result<(), AuraError> {
    let created_at = time
        .physical_time()
        .await
        .map_err(|e| AuraError::internal(format!("Failed to fetch physical time: {e}")))?
        .ts_ms;

    let config = AccountConfig {
        authority_id: hex::encode(authority_id.to_bytes()),
        context_id: hex::encode(context_id.to_bytes()),
        display_name,
        created_at,
    };

    let content = serde_json::to_vec_pretty(&config)
        .map_err(|e| AuraError::internal(format!("Failed to serialize account config: {e}")))?;

    storage
        .store(ACCOUNT_FILENAME, content)
        .await
        .map_err(|e| AuraError::internal(format!("Failed to write account config: {e}")))?;

    Ok(())
}

/// Create a new account and save to disk
///
/// Called when user completes the account setup modal.
/// The base_path should be mode-specific (.aura or .aura-demo).
pub async fn create_account(
    base_path: &Path,
    device_id_str: &str,
    display_name: &str,
) -> Result<(AuthorityId, ContextId), AuraError> {
    let storage = open_bootstrap_storage(base_path);
    let time = PhysicalTimeHandler::new();

    // Create new account with deterministic IDs based on device_id
    // This ensures the same device_id always creates the same account
    let authority_entropy = aura_core::hash::hash(format!("authority:{device_id_str}").as_bytes());
    let context_entropy = aura_core::hash::hash(format!("context:{device_id_str}").as_bytes());

    let authority_id = AuthorityId::new_from_entropy(authority_entropy);
    let context_id = ContextId::new_from_entropy(context_entropy);

    // Persist to storage using effect-backed handlers.
    persist_account_config(
        &storage,
        &time,
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
/// * `base_path` - Data directory for account storage (mode-specific)
/// * `recovered_authority_id` - The ORIGINAL authority_id reconstructed by guardians
/// * `recovered_context_id` - Optional context_id (generated deterministically if None)
///
/// # Returns
/// * The authority and context IDs written to disk
pub async fn restore_recovered_account(
    base_path: &Path,
    recovered_authority_id: AuthorityId,
    recovered_context_id: Option<ContextId>,
) -> Result<(AuthorityId, ContextId), AuraError> {
    let storage = open_bootstrap_storage(base_path);
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

    persist_account_config(&storage, &time, recovered_authority_id, context_id, None).await?;

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
/// * `base_path` - Data directory containing account and journal files (mode-specific)
/// * `device_id` - Optional device ID to include in backup metadata
///
/// # Returns
/// * Portable backup code string
pub async fn export_account_backup(
    base_path: &Path,
    device_id: Option<&str>,
) -> Result<String, AuraError> {
    let storage = open_bootstrap_storage(base_path);
    let time = PhysicalTimeHandler::new();

    let Some(account_bytes) = storage
        .retrieve(ACCOUNT_FILENAME)
        .await
        .map_err(|e| AuraError::internal(format!("Failed to read account config: {e}")))?
    else {
        return Err(AuraError::internal("No account exists to backup"));
    };

    let account: AccountConfig = serde_json::from_slice(&account_bytes)
        .map_err(|e| AuraError::internal(format!("Failed to parse account config: {e}")))?;

    let journal = storage
        .retrieve(JOURNAL_FILENAME)
        .await
        .map_err(|e| AuraError::internal(format!("Failed to read journal: {e}")))?
        .and_then(|b| String::from_utf8(b).ok());

    let backup_at = time
        .physical_time()
        .await
        .map_err(|e| AuraError::internal(format!("Failed to fetch physical time: {e}")))?
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
        .map_err(|e| AuraError::internal(format!("Failed to serialize backup: {e}")))?;

    // Encode as base64 with prefix
    use base64::Engine;
    let encoded = base64::engine::general_purpose::STANDARD.encode(json.as_bytes());

    Ok(format!("{BACKUP_PREFIX}{encoded}"))
}

/// Import and restore account from backup code
///
/// # Arguments
/// * `base_path` - Data directory to restore account to (mode-specific)
/// * `backup_code` - The backup code from `export_account_backup`
/// * `overwrite` - If true, overwrite existing account; if false, fail if account exists
///
/// # Returns
/// * The restored authority and context IDs
pub async fn import_account_backup(
    base_path: &Path,
    backup_code: &str,
    overwrite: bool,
) -> Result<(AuthorityId, ContextId), AuraError> {
    let storage = open_bootstrap_storage(base_path);

    // Parse backup code
    if !backup_code.starts_with(BACKUP_PREFIX) {
        return Err(AuraError::internal(format!(
            "Invalid backup code format (expected prefix '{BACKUP_PREFIX}')"
        )));
    }

    let encoded = &backup_code[BACKUP_PREFIX.len()..];

    // Decode base64
    use base64::Engine;
    let json_bytes = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .map_err(|e| AuraError::internal(format!("Invalid backup code encoding: {e}")))?;

    let json = String::from_utf8(json_bytes)
        .map_err(|e| AuraError::internal(format!("Invalid backup code UTF-8: {e}")))?;

    // Parse backup structure
    let backup: AccountBackup = serde_json::from_str(&json)
        .map_err(|e| AuraError::internal(format!("Invalid backup format: {e}")))?;

    // Validate version
    if backup.version > BACKUP_VERSION {
        return Err(AuraError::internal(format!(
            "Backup version {} is newer than supported version {}",
            backup.version, BACKUP_VERSION
        )));
    }

    // Parse authority ID
    let authority_bytes: [u8; 16] = hex::decode(&backup.account.authority_id)
        .map_err(|e| AuraError::internal(format!("Invalid authority_id in backup: {e}")))?
        .try_into()
        .map_err(|_| AuraError::internal("Invalid authority_id length in backup"))?;
    let authority_id = AuthorityId::from_uuid(uuid::Uuid::from_bytes(authority_bytes));

    // Parse context ID
    let context_bytes: [u8; 16] = hex::decode(&backup.account.context_id)
        .map_err(|e| AuraError::internal(format!("Invalid context_id in backup: {e}")))?
        .try_into()
        .map_err(|_| AuraError::internal("Invalid context_id length in backup"))?;
    let context_id = ContextId::from_uuid(uuid::Uuid::from_bytes(context_bytes));

    // Check for existing account
    if storage
        .exists(ACCOUNT_FILENAME)
        .await
        .map_err(|e| AuraError::internal(format!("Failed to check account existence: {e}")))?
        && !overwrite
    {
        return Err(AuraError::internal(
            "Account already exists. Use overwrite=true to replace.",
        ));
    }

    // Write account configuration.
    let account_content = serde_json::to_vec_pretty(&backup.account)
        .map_err(|e| AuraError::internal(format!("Failed to serialize account config: {e}")))?;

    storage
        .store(ACCOUNT_FILENAME, account_content)
        .await
        .map_err(|e| AuraError::internal(format!("Failed to write account config: {e}")))?;

    // Write journal if present in backup
    if let Some(ref journal_content) = backup.journal {
        storage
            .store(JOURNAL_FILENAME, journal_content.as_bytes().to_vec())
            .await
            .map_err(|e| AuraError::internal(format!("Failed to write journal: {e}")))?;
    }

    Ok((authority_id, context_id))
}

/// Demo seed for deterministic simulation
const DEMO_SEED: u64 = 2024;

/// Handle TUI launch
pub async fn handle_tui(args: &TuiArgs) -> crate::error::TerminalResult<()> {
    let stdio = PreFullscreenStdio::new();

    // Determine mode from args
    let mode = if args.demo {
        stdio.println(format_args!("Starting Aura TUI (Demo Mode)"));
        stdio.println(format_args!("============================="));
        stdio.println(format_args!(
            "Demo mode runs a real agent with simulated effects."
        ));
        stdio.println(format_args!("Seed: {DEMO_SEED} (deterministic)"));
        stdio.newline();
        TuiMode::Demo { seed: DEMO_SEED }
    } else {
        TuiMode::Production
    };

    // Default device ID for demo mode
    let device_id = args
        .device_id
        .as_deref()
        .or(if args.demo { Some("demo:bob") } else { None });

    // Path resolution happens in handle_tui_launch via resolve_storage_path
    handle_tui_launch(stdio, args.data_dir.as_deref(), device_id, mode).await
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
    stdio.println(format_args!("Starting Aura TUI ({mode_str})"));
    stdio.println(format_args!("================"));

    // Use the single source of truth for path resolution
    let base_path = resolve_storage_path(data_dir, mode);
    let storage = Arc::new(open_bootstrap_storage(&base_path));

    // In demo mode, clear storage so users go through account creation
    // This is ONLY done in demo mode - production directory is NEVER touched
    if matches!(mode, TuiMode::Demo { .. }) {
        cleanup_demo_storage(storage.as_ref(), &base_path).await;
    }

    // Initialize tracing for TUI into storage (avoid stderr corruption in fullscreen).
    // Safe to call multiple times; only the first init wins.
    init_tui_tracing(storage.clone(), mode);

    // Determine device ID
    let device_id = device_id_str
        .map(|id| crate::ids::device_id(id))
        .unwrap_or_else(|| crate::ids::device_id("tui:production-device"));

    stdio.println(format_args!("Data directory: {}", base_path.display()));
    stdio.println(format_args!("Device ID: {device_id}"));
    std::env::set_var("AURA_DEMO_BOB_DEVICE_ID", device_id.to_string());

    // Determine device ID string for account derivation
    let device_id_for_account = device_id_str.unwrap_or("tui:production-device");

    // Try to load existing account, or use placeholders if no account exists
    let (authority_id, context_id, has_existing_account) =
        match try_load_account(storage.as_ref()).await? {
            AccountLoadResult::Loaded { authority, context } => {
                stdio.println(format_args!("Authority: {authority}"));
                stdio.println(format_args!("Context: {context}"));
                (authority, context, true)
            }
            AccountLoadResult::NotFound => {
                // Use placeholder IDs - the TUI will show account setup modal
                let (authority, context) = create_placeholder_ids(device_id_for_account);
                stdio.println(format_args!("No existing account - will show setup modal"));
                stdio.println(format_args!("Placeholder Authority: {authority}"));
                stdio.println(format_args!("Placeholder Context: {context}"));
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

    // In demo mode, create simulator first to get shared transport wiring (Bob + Alice + Carol).
    #[cfg(feature = "development")]
    let demo_simulator_for_bob = match mode {
        TuiMode::Demo { seed } => {
            stdio.println(format_args!(
                "Creating demo simulator for shared transport..."
            ));
            let sim = DemoSimulator::new(seed, base_path.clone(), authority_id, context_id)
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
            .map_err(|e| AuraError::internal(format!("Failed to create agent: {e}")))?,
        TuiMode::Demo { seed } => {
            stdio.println(format_args!("Using simulation agent with seed: {seed}"));

            #[cfg(feature = "development")]
            {
                // Use shared transport wiring from simulator
                let shared_transport = demo_simulator_for_bob
                    .as_ref()
                    .map(|sim| sim.shared_transport())
                    .expect("Simulator should be created in demo mode");

                stdio.println(format_args!(
                    "Creating Bob's agent with shared transport..."
                ));
                AgentBuilder::new()
                    .with_config(agent_config)
                    .with_authority(authority_id)
                    .build_simulation_async_with_shared_transport(
                        seed,
                        &effect_ctx,
                        shared_transport,
                    )
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
                        AuraError::internal(format!("Failed to create simulation agent: {e}"))
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
        .map_err(|e| AuraError::internal(format!("Failed to create AppCore: {e}")))?;

    let app_core = Arc::new(RwLock::new(app_core));

    let app_core = InitializedAppCore::new(app_core).await?;

    if let Err(e) =
        aura_app::ui::workflows::settings::refresh_settings_from_runtime(app_core.raw()).await
    {
        stdio.eprintln(format_args!("Warning: Failed to refresh settings: {e}"));
    }

    stdio.println(format_args!(
        "AppCore initialized (with runtime bridge and reactive signals)"
    ));

    #[cfg(feature = "development")]
    let mut simulator: Option<DemoSimulator> = match mode {
        TuiMode::Demo { .. } => {
            stdio.println(format_args!("Starting demo simulator..."));
            let mut sim = demo_simulator_for_bob.expect("Simulator should exist in demo mode");
            sim.start()
                .await
                .map_err(|e| AuraError::internal(format!("Failed to start simulator: {}", e)))?;

            stdio.println(format_args!("Alice online: {}", sim.alice_authority()));
            stdio.println(format_args!("Carol online: {}", sim.carol_authority()));
            stdio.println(format_args!("Mobile online: {}", sim.mobile_authority()));
            std::env::set_var("AURA_DEMO_DEVICE_ID", sim.mobile_device_id().to_string());

            // Refresh UI-facing signals from the runtime, including connection status.
            if let Err(e) = aura_app::ui::workflows::system::refresh_account(app_core.raw()).await {
                tracing::warn!("Demo: Failed to refresh account state: {}", e);
            }

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

            // Auto-respond to Bob's AMP messages from demo peers.
            let app_core_for_replies = app_core.raw().clone();
            let bob_authority = authority_id;
            let alice_agent = sim.alice_agent();
            let carol_agent = sim.carol_agent();
            tokio::spawn(async move {
                use aura_app::ui::signals::{CHAT_SIGNAL, HOMES_SIGNAL};
                use aura_core::effects::amp::ChannelSendParams;
                use aura_core::identifiers::ContextId;
                use aura_core::EffectContext;
                use std::collections::HashSet;

                let mut chat_stream = {
                    let core = app_core_for_replies.read().await;
                    core.subscribe(&*CHAT_SIGNAL)
                };

                let mut seen_messages: HashSet<String> = HashSet::new();

                loop {
                    let chat_state = match chat_stream.recv().await {
                        Ok(state) => state,
                        Err(_) => break,
                    };

                    for msg in &chat_state.messages {
                        if msg.sender_id != bob_authority {
                            continue;
                        }
                        if !seen_messages.insert(msg.id.clone()) {
                            continue;
                        }

                        let context_id = {
                            let core = app_core_for_replies.read().await;
                            let homes = core.read(&*HOMES_SIGNAL).await.unwrap_or_default();
                            homes
                                .home_state(&msg.channel_id)
                                .map(|home| home.context_id)
                                .unwrap_or_else(|| {
                                    EffectContext::with_authority(bob_authority).context_id()
                                })
                        };

                        let reply = format!("received: {}", msg.content);
                        for agent in [&alice_agent, &carol_agent] {
                            let params = ChannelSendParams {
                                context: context_id,
                                channel: msg.channel_id,
                                sender: agent.authority_id(),
                                plaintext: reply.as_bytes().to_vec(),
                                reply_to: None,
                            };

                            if let Err(err) = agent.runtime().effects().send_message(params).await {
                                tracing::warn!(
                                    agent = %agent.authority_id(),
                                    error = %err,
                                    "Demo auto-reply send failed"
                                );
                            }
                        }
                    }
                }
            });
            stdio.println(format_args!("Demo auto-reply loop started"));

            Some(sim)
        }
        TuiMode::Production => None,
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
            let demo_mobile_agent = simulator
                .as_ref()
                .map(|sim| sim.mobile_agent())
                .expect("Simulator should exist in demo mode");
            let demo_mobile_device_id = simulator
                .as_ref()
                .map(|sim| sim.mobile_device_id().to_string())
                .expect("Simulator should exist in demo mode");
            let builder = IoContext::builder()
                .with_app_core(app_core)
                .with_base_path(base_path.clone())
                .with_device_id(device_id_for_account.to_string())
                .with_mode(mode)
                .with_existing_account(has_existing_account)
                .with_demo_hints(hints)
                .with_demo_mobile_agent(demo_mobile_agent)
                .with_demo_mobile_device_id(demo_mobile_device_id);

            builder.build().map_err(|e| {
                crate::error::TerminalError::Config(format!("IoContext build failed: {e}"))
            })?
        }
        TuiMode::Production => IoContext::builder()
            .with_app_core(app_core.clone())
            .with_base_path(base_path.clone())
            .with_device_id(device_id_for_account.to_string())
            .with_mode(mode)
            .with_existing_account(has_existing_account)
            .build()
            .map_err(|e| {
                crate::error::TerminalError::Config(format!("IoContext build failed: {e}"))
            })?,
    };

    #[cfg(not(feature = "development"))]
    let ctx = IoContext::builder()
        .with_app_core(app_core.clone())
        .with_base_path(base_path.clone())
        .with_device_id(device_id_for_account.to_string())
        .with_mode(mode)
        .with_existing_account(has_existing_account)
        .build()
        .map_err(|e| crate::error::TerminalError::Config(format!("IoContext build failed: {e}")))?;

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
    let result = result.map_err(|e| AuraError::internal(format!("TUI failed: {e}")));

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

struct StorageLogWriter {
    sender: mpsc::Sender<Vec<u8>>,
}

impl io::Write for StorageLogWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self.sender.try_send(buf.to_vec()) {
            Ok(()) => {}
            Err(mpsc::error::TrySendError::Full(_)) => {
                // Drop log chunks when the queue is saturated to avoid unbounded memory growth.
            }
            Err(mpsc::error::TrySendError::Closed(_)) => {
                return Err(io::Error::new(
                    io::ErrorKind::BrokenPipe,
                    "log channel closed",
                ));
            }
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[allow(clippy::needless_pass_by_value)] // TuiMode is small and matched on
fn init_tui_tracing(storage: Arc<dyn StorageCoreEffects>, mode: TuiMode) {
    // Allow forcing stdio tracing for debugging.
    if std::env::var("AURA_TUI_ALLOW_STDIO").ok().as_deref() == Some("1") {
        return;
    }

    let default_name = match mode {
        TuiMode::Production => "aura-tui.log",
        TuiMode::Demo { .. } => "aura-tui-demo.log",
    };

    let log_key = std::env::var("AURA_TUI_LOG_PATH")
        .ok()
        .filter(|path| !path.trim().is_empty())
        .unwrap_or_else(|| format!("{TUI_LOG_KEY_PREFIX}/{default_name}"));

    let (tx, mut rx) = mpsc::channel::<Vec<u8>>(TUI_LOG_QUEUE_CAPACITY);
    let storage_task = storage.clone();
    let log_key_task = log_key;

    tokio::spawn(async move {
        let mut buffer: Vec<u8> = Vec::new();
        while let Some(chunk) = rx.recv().await {
            buffer.extend_from_slice(&chunk);
            if buffer.len() > MAX_TUI_LOG_BYTES {
                let excess = buffer.len() - MAX_TUI_LOG_BYTES;
                buffer.drain(0..excess);
            }
            if let Err(err) = storage_task.store(&log_key_task, buffer.clone()).await {
                tracing::warn!(error = %err, "Failed to persist TUI log chunk");
            }
        }
    });

    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let make_writer = {
        let sender = tx;
        move || StorageLogWriter {
            sender: sender.clone(),
        }
    };

    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_ansi(false)
        .with_target(false)
        .with_writer(make_writer)
        .try_init();
}
