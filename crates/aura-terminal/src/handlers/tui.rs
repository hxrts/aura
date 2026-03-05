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
use std::time::Duration;

// Import app types from aura-app (pure layer)
use aura_app::ui::prelude::*;
// Import portable account types and ID derivation functions
use aura_app::ui::types::{AccountBackup, AccountConfig};
use aura_app::ui::workflows::account::{
    derive_authority_id, derive_context_id, derive_recovered_context_id, parse_backup_code,
};
// Import agent types from aura-agent (runtime layer)
use async_lock::RwLock;
use aura_agent::core::config::{NetworkConfig, StorageConfig};
use aura_agent::{AgentBuilder, AgentConfig, EffectContext, SyncManagerConfig};
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
use crate::demo::{spawn_amp_inbox_listener, DemoSimulator, EchoPeer};
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

// AccountConfig is now imported from aura_app::ui::types

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

// Storage constants re-exported from portable aura-app layer
pub use aura_app::ui::types::{
    ACCOUNT_FILENAME, JOURNAL_FILENAME, MAX_TUI_LOG_BYTES, TUI_LOG_KEY_PREFIX,
    TUI_LOG_QUEUE_CAPACITY,
};

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

    Ok(AccountLoadResult::Loaded {
        authority: config.authority_id,
        context: config.context_id,
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
    nickname_suggestion: Option<String>,
) -> Result<(), AuraError> {
    let created_at = time
        .physical_time()
        .await
        .map_err(|e| AuraError::internal(format!("Failed to fetch physical time: {e}")))?
        .ts_ms;

    let config = AccountConfig {
        authority_id,
        context_id,
        nickname_suggestion,
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
///
/// Uses portable ID derivation from aura_app::workflows::account.
pub async fn create_account(
    base_path: &Path,
    device_id_str: &str,
    nickname_suggestion: &str,
) -> Result<(AuthorityId, ContextId), AuraError> {
    let storage = open_bootstrap_storage(base_path);
    let time = PhysicalTimeHandler::new();

    // Use portable ID derivation functions from aura-app
    let authority_id = derive_authority_id(device_id_str);
    let context_id = derive_context_id(device_id_str);

    // Persist to storage using effect-backed handlers.
    persist_account_config(
        &storage,
        &time,
        authority_id,
        context_id,
        Some(nickname_suggestion.to_string()),
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
/// Uses portable ID derivation from aura_app::workflows::account.
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

    // For context, either use the recovered one or derive using portable function
    let context_id = recovered_context_id
        .unwrap_or_else(|| derive_recovered_context_id(&recovered_authority_id));

    persist_account_config(&storage, &time, recovered_authority_id, context_id, None).await?;

    Ok((recovered_authority_id, context_id))
}

// =============================================================================
// Account Backup/Export
// =============================================================================
// AccountBackup, AccountConfig, BACKUP_VERSION, and BACKUP_PREFIX are now
// imported from aura_app::ui::types for portable backup operations.

/// Export account to a portable backup code
///
/// The backup code is a base64-encoded JSON blob with a prefix for easy identification.
/// Format: `aura:backup:v1:<base64>`
///
/// Uses portable AccountBackup from aura_app::ui::types.
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

    // Create backup structure using portable type
    let backup = AccountBackup::new(account, journal, backup_at, device_id.map(String::from));

    // Encode using portable method
    backup
        .encode()
        .map_err(|e| AuraError::internal(format!("Failed to encode backup: {e}")))
}

/// Import and restore account from backup code
///
/// Uses portable parse_backup_code from aura_app::workflows::account.
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

    // Parse and validate backup using portable function
    let backup = parse_backup_code(backup_code)?;

    let authority_id = backup.account.authority_id;
    let context_id = backup.account.context_id;

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
    if let Some(journal_content) = &backup.journal {
        storage
            .store(JOURNAL_FILENAME, journal_content.as_bytes().to_vec())
            .await
            .map_err(|e| AuraError::internal(format!("Failed to write journal: {e}")))?;
    }

    Ok((authority_id, context_id))
}

// Demo seed re-exported from portable aura-app layer
use aura_app::ui::workflows::demo_config::DEMO_SEED_2024 as DEMO_SEED;

#[cfg(feature = "development")]
async fn seed_realistic_demo_world(
    app_core: &Arc<RwLock<AppCore>>,
    bob_agent: &Arc<aura_agent::AuraAgent>,
    simulator: &DemoSimulator,
) -> crate::error::TerminalResult<()> {
    use std::collections::HashMap;

    use aura_app::ui::signals::{
        HOMES_SIGNAL, HOMES_SIGNAL_NAME, NEIGHBORHOOD_SIGNAL, NEIGHBORHOOD_SIGNAL_NAME,
    };
    use aura_app::ui::types::{HomeMember, HomeRole, NeighborHome, OneHopLinkType};
    use aura_app::ui::workflows::context::{
        add_home_to_neighborhood, create_home, create_neighborhood,
    };
    use aura_app::ui::workflows::signals::{emit_signal, read_signal_or_default};

    // Social peers that Bob already exchanged contacts with in the richer demo.
    let prelinked_contacts = ["Dave", "Grace", "Judy", "Olivia", "Peggy", "Sybil"];

    // Homes are grouped into three neighborhood clusters.
    let home_specs: Vec<(
        &'static str,
        &'static str,
        Vec<&'static str>,
        OneHopLinkType,
        u32,
    )> = vec![
        (
            "Northside",
            "Maple House",
            vec!["Dave", "Eve", "Frank"],
            OneHopLinkType::Direct,
            6,
        ),
        (
            "Northside",
            "Cedar House",
            vec!["Grace", "Heidi", "Ivan"],
            OneHopLinkType::Direct,
            5,
        ),
        (
            "Riverside",
            "Harbor House",
            vec!["Judy", "Mallory"],
            OneHopLinkType::TwoHop,
            4,
        ),
        (
            "Riverside",
            "Foundry House",
            vec!["Niaj", "Olivia"],
            OneHopLinkType::TwoHop,
            4,
        ),
        (
            "Hillside",
            "Orchard House",
            vec!["Peggy", "Rupert"],
            OneHopLinkType::Distant,
            3,
        ),
        (
            "Hillside",
            "Lantern House",
            vec!["Sybil", "Dave"],
            OneHopLinkType::Distant,
            3,
        ),
    ];

    let mut peer_authorities = HashMap::new();
    for profile in simulator.social_peer_profiles() {
        peer_authorities.insert(profile.name, profile.authority_id);
    }
    peer_authorities.insert("Alice".to_string(), simulator.alice_authority());
    peer_authorities.insert("Carol".to_string(), simulator.carol_authority());

    // Seed Bob-side contact relationships so the Contacts screen starts populated.
    let now_ms = bob_agent.runtime().effects().current_timestamp_ms().await;
    let contacts_to_add: Vec<(String, &str, u64)> = prelinked_contacts
        .iter()
        .enumerate()
        .filter_map(|(idx, name)| {
            peer_authorities
                .get(*name)
                .map(|peer_id| (peer_id.to_string(), *name, now_ms + idx as u64))
        })
        .collect();

    if !contacts_to_add.is_empty() {
        let contacts_refs: Vec<(&str, &str, u64)> = contacts_to_add
            .iter()
            .map(|(id, name, ts)| (id.as_str(), *name, *ts))
            .collect();
        aura_app::ui::workflows::contacts::add_contacts_batch(app_core, &contacts_refs).await?;
    }

    let _ = create_neighborhood(app_core, "Tri-Neighborhood Demo".to_string()).await?;

    let mut created_homes = Vec::with_capacity(home_specs.len());
    for (cluster, home_name, members, hop, shared_contacts) in home_specs {
        let display_name = format!("{cluster} · {home_name}");
        let home_id = create_home(
            app_core,
            Some(display_name.clone()),
            Some(format!("Demo home in the {cluster} cluster")),
        )
        .await?;
        add_home_to_neighborhood(app_core, &home_id.to_string()).await?;
        created_homes.push((home_id, display_name, members, hop, shared_contacts));
    }

    let mut homes_state = read_signal_or_default(app_core, &*HOMES_SIGNAL).await;
    let mut neighborhood_state = read_signal_or_default(app_core, &*NEIGHBORHOOD_SIGNAL).await;

    for (home_id, display_name, members, hop, shared_contacts) in &created_homes {
        if let Some(home) = homes_state.home_mut(home_id) {
            for (idx, member_name) in members.iter().enumerate() {
                let Some(member_id) = peer_authorities.get(*member_name) else {
                    continue;
                };
                if home.members.iter().any(|member| member.id == *member_id) {
                    continue;
                }

                let role = if idx == 0 {
                    HomeRole::Member
                } else {
                    HomeRole::Participant
                };
                home.add_member(HomeMember {
                    id: *member_id,
                    name: (*member_name).to_string(),
                    role,
                    is_online: true,
                    joined_at: now_ms + idx as u64 + 1,
                    last_seen: Some(now_ms + idx as u64 + 1),
                    storage_allocated: aura_app::ui::types::MEMBER_ALLOCATION,
                });
            }

            if *home_id != neighborhood_state.home_home_id {
                neighborhood_state.add_neighbor(NeighborHome {
                    id: *home_id,
                    name: display_name.clone(),
                    one_hop_link: *hop,
                    shared_contacts: *shared_contacts,
                    member_count: Some(home.member_count),
                    can_traverse: true,
                });
            }
        }
    }

    neighborhood_state.neighborhood_id = Some(format!("demo-topology-{}", simulator.seed()));
    neighborhood_state.neighborhood_name = Some("Tri-Neighborhood Demo".to_string());
    neighborhood_state.set_member_homes(created_homes.iter().map(|(id, ..)| *id));

    emit_signal(app_core, &*HOMES_SIGNAL, homes_state, HOMES_SIGNAL_NAME).await?;
    emit_signal(
        app_core,
        &*NEIGHBORHOOD_SIGNAL,
        neighborhood_state,
        NEIGHBORHOOD_SIGNAL_NAME,
    )
    .await?;

    aura_app::ui::workflows::system::refresh_account(app_core).await?;
    Ok(())
}

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
    handle_tui_launch(
        stdio,
        args.data_dir.as_deref(),
        device_id,
        args.bind_address.as_deref(),
        mode,
    )
    .await
}

/// Launch the TUI with the specified mode
async fn handle_tui_launch(
    stdio: PreFullscreenStdio,
    data_dir: Option<&str>,
    device_id_str: Option<&str>,
    bind_address: Option<&str>,
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
    if let Some(addr) = bind_address {
        stdio.println(format_args!("Bind address: {addr}"));
    }
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
        network: NetworkConfig {
            bind_address: bind_address.unwrap_or("0.0.0.0:0").to_string(),
            ..NetworkConfig::default()
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
    let sync_config = SyncManagerConfig {
        auto_sync_interval: Duration::from_secs(2),
        ..SyncManagerConfig::default()
    };
    let agent = match mode {
        TuiMode::Production => AgentBuilder::new()
            .with_config(agent_config)
            .with_authority(authority_id)
            .with_sync_config(sync_config.clone())
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
                    .ok_or_else(|| AuraError::internal("Simulator not available in demo mode"))?;

                stdio.println(format_args!(
                    "Creating Bob's agent with shared transport..."
                ));
                AgentBuilder::new()
                    .with_config(agent_config)
                    .with_authority(authority_id)
                    .with_sync_config(sync_config.clone())
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
                    .with_sync_config(sync_config.clone())
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

    // Process inbound envelopes continuously in both production and demo modes.
    // Without this, production TUI sessions may never drain transport inboxes,
    // causing cross-instance chat/invitation updates to stall until a command
    // opportunistically polls the runtime bridge.
    let ceremony_agent = agent.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(tokio::time::Duration::from_millis(500));
        loop {
            interval.tick().await;
            if let Err(e) = ceremony_agent.process_ceremony_acceptances().await {
                tracing::debug!("Error processing ceremony acceptances: {}", e);
            }
        }
    });

    #[cfg(feature = "development")]
    let mut simulator: Option<DemoSimulator> = match mode {
        TuiMode::Demo { .. } => {
            stdio.println(format_args!("Starting demo simulator..."));
            let mut sim = demo_simulator_for_bob
                .ok_or_else(|| AuraError::internal("Simulator not available in demo mode"))?;
            sim.enable_realistic_world(base_path.clone())
                .await
                .map_err(|e| {
                    AuraError::internal(format!("Failed to build realistic demo world: {e}"))
                })?;
            sim.start()
                .await
                .map_err(|e| AuraError::internal(format!("Failed to start simulator: {}", e)))?;

            stdio.println(format_args!("Alice online: {}", sim.alice_authority()));
            stdio.println(format_args!("Carol online: {}", sim.carol_authority()));
            stdio.println(format_args!("Mobile online: {}", sim.mobile_authority()));
            std::env::set_var("AURA_DEMO_DEVICE_ID", sim.mobile_device_id().to_string());

            seed_realistic_demo_world(app_core.raw(), &agent, &sim)
                .await
                .map_err(|e| AuraError::internal(format!("Failed to seed demo world: {e}")))?;

            // Refresh UI-facing signals from the runtime, including connection status.
            if let Err(e) = aura_app::ui::workflows::system::refresh_account(app_core.raw()).await {
                tracing::warn!("Demo: Failed to refresh account state: {}", e);
            }

            Some(sim)
        }
        TuiMode::Production => None,
    };

    #[cfg(feature = "development")]
    if let TuiMode::Demo { .. } = mode {
        if let Some(sim) = &simulator {
            let effects = agent.runtime().effects();

            // Create echo peers for Alice and Carol
            let peers = vec![
                EchoPeer {
                    authority_id: sim.alice_authority(),
                    name: "Alice".to_string(),
                },
                EchoPeer {
                    authority_id: sim.carol_authority(),
                    name: "Carol".to_string(),
                },
            ];

            let _amp_inbox_handle = spawn_amp_inbox_listener(effects, authority_id, peers);
        }
    }

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
                .ok_or_else(|| {
                    crate::error::TerminalError::Operation(
                        "Simulator not available in demo mode".into(),
                    )
                })?;
            let demo_mobile_device_id = simulator
                .as_ref()
                .map(|sim| sim.mobile_device_id().to_string())
                .ok_or_else(|| {
                    crate::error::TerminalError::Operation(
                        "Simulator not available in demo mode".into(),
                    )
                })?;
            let demo_mobile_authority_id = simulator
                .as_ref()
                .map(|sim| sim.mobile_authority().to_string())
                .ok_or_else(|| {
                    crate::error::TerminalError::Operation(
                        "Simulator not available in demo mode".into(),
                    )
                })?;
            let builder = IoContext::builder()
                .with_app_core(app_core)
                .with_base_path(base_path.clone())
                .with_device_id(device_id_for_account.to_string())
                .with_mode(mode)
                .with_existing_account(has_existing_account)
                .with_demo_hints(hints)
                .with_demo_mobile_agent(demo_mobile_agent)
                .with_demo_mobile_device_id(demo_mobile_device_id)
                .with_demo_mobile_authority_id(demo_mobile_authority_id);

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
    // Allow forcing stdio tracing for debugging (writes to stderr).
    if std::env::var("AURA_TUI_ALLOW_STDIO").ok().as_deref() == Some("1") {
        let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
        let _ = tracing_subscriber::fmt()
            .with_env_filter(filter)
            .with_ansi(false)
            .with_target(true)
            .with_writer(std::io::stderr)
            .try_init();
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
