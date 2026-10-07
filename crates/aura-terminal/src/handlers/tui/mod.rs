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
use std::os::unix::process::CommandExt;
use std::panic::AssertUnwindSafe;
use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;
use std::time::Duration;

use async_lock::RwLock;
use aura_agent::core::config::{NetworkConfig, StorageConfig};
use aura_agent::{
    core::default_context_id_for_authority, AgentBuilder, AgentConfig, AuraAgent,
    BootstrapBrokerConfig, EffectContext, SyncManagerConfig,
};
use aura_app::ui::prelude::*;
use aura_app::ui::types::{BootstrapEvent, BootstrapEventKind, BootstrapSurface};
use aura_app::ui::workflows::account::initialize_runtime_account;
use aura_app::ui::workflows::context as context_workflows;
use aura_app::ui::workflows::demo_config::DEMO_SEED_2024 as DEMO_SEED;
use aura_core::effects::ExecutionMode;
use aura_core::types::identifiers::{AuthorityId, ContextId, DeviceId};
use aura_core::AuraError;
use futures::FutureExt;

use crate::cli::tui::TuiArgs;
#[cfg(feature = "development")]
use crate::demo::{spawn_amp_inbox_listener, DemoHints, DemoSimulator, EchoPeer};
use crate::env::{
    bootstrap_broker_override as bootstrap_broker_env_override,
    harness_lan_discovery_override as harness_lan_discovery_env_override,
};
use crate::handlers::tui_stdio::{during_fullscreen, PreFullscreenStdio};
use crate::tui::{
    context::{InitializedAppCore, IoContext, ShellExitIntent},
    screens::run_app_with_context,
    tasks::UiTaskOwner,
};

mod account;
#[cfg(feature = "development")]
mod demo_mode;
#[path = "tracing.rs"]
mod tui_tracing;

use account::{
    cleanup_demo_storage, clear_pending_account_bootstrap, load_pending_account_bootstrap,
    load_selected_runtime_identity, persist_selected_authority, try_load_account,
    wait_for_persisted_account,
};
#[cfg(feature = "development")]
use demo_mode::seed_realistic_demo_world;
use tui_tracing::init_tui_tracing;

pub use account::{
    create_account, create_account_in, export_account_backup, import_account_backup,
    persist_completed_enrollment_runtime_identity, restore_recovered_account,
    stage_account_for_bootstrap, ProfileStore,
};

pub use aura_app::ui::types::{
    ACCOUNT_FILENAME, JOURNAL_FILENAME, MAX_TUI_LOG_BYTES, TUI_LOG_KEY_PREFIX,
    TUI_LOG_QUEUE_CAPACITY,
};

/// Whether the TUI is running in demo or production mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TuiMode {
    /// Production mode with real network/storage.
    Production,
    /// Demo mode with simulated effects and peer agents.
    Demo { seed: u64 },
}

impl TuiMode {
    fn display_name(self) -> &'static str {
        match self {
            Self::Production => "Production",
            Self::Demo { .. } => "Demo (Simulation)",
        }
    }

    fn execution_mode(self) -> ExecutionMode {
        match self {
            Self::Production => ExecutionMode::Production,
            Self::Demo { seed } => ExecutionMode::Simulation { seed },
        }
    }

    fn default_device_id(self) -> Option<&'static str> {
        match self {
            Self::Production => None,
            Self::Demo { .. } => Some("demo:bob"),
        }
    }

    fn log_filename(self) -> &'static str {
        match self {
            Self::Production => "aura-tui.log",
            Self::Demo { .. } => "aura-tui-demo.log",
        }
    }

    pub(crate) fn is_demo(self) -> bool {
        matches!(self, Self::Demo { .. })
    }
}

/// Account loading result.
pub enum AccountLoadResult {
    /// Account loaded from existing file.
    Loaded {
        authority: AuthorityId,
        context: ContextId,
        nickname_suggestion: Option<String>,
    },
    /// No account exists and setup is still required.
    NotFound,
}

#[derive(Debug, Clone, Copy)]
struct TuiLaunchRequest<'a> {
    data_dir: Option<&'a str>,
    requested_device_id: Option<&'a str>,
    bind_address: Option<&'a str>,
    mode: TuiMode,
}

impl<'a> TuiLaunchRequest<'a> {
    fn from_args(stdio: &PreFullscreenStdio, args: &'a TuiArgs) -> Self {
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

        Self {
            data_dir: args.data_dir.as_deref(),
            requested_device_id: args
                .device_id
                .as_deref()
                .or_else(|| mode.default_device_id()),
            bind_address: args.bind_address.as_deref(),
            mode,
        }
    }

    fn resolve(self) -> ResolvedTuiLaunch {
        ResolvedTuiLaunch {
            mode: self.mode,
            base_path: resolve_storage_path(self.data_dir, self.mode),
            configured_device_id: self
                .requested_device_id
                .map(crate::ids::device_id)
                .unwrap_or_else(|| crate::ids::device_id("tui:production-device")),
            bind_address: self.bind_address.map(str::to_string),
        }
    }
}

#[derive(Debug, Clone)]
struct ResolvedTuiLaunch {
    mode: TuiMode,
    base_path: PathBuf,
    configured_device_id: DeviceId,
    bind_address: Option<String>,
}

impl ResolvedTuiLaunch {
    fn print_startup(&self, stdio: &mut PreFullscreenStdio) {
        stdio.println(format_args!(
            "Starting Aura TUI ({})",
            self.mode.display_name()
        ));
        stdio.println(format_args!("================"));
        stdio.println(format_args!("Data directory: {}", self.base_path.display()));
        stdio.println(format_args!("Device ID: {}", self.configured_device_id));
        if let Some(address) = self.bind_address.as_deref() {
            stdio.println(format_args!("Bind address: {address}"));
        }
    }

    fn app_config(&self) -> AppConfig {
        AppConfig {
            data_dir: self.base_path.to_string_lossy().to_string(),
            debug: false,
            journal_path: None,
        }
    }

    fn runtime_spec(
        &self,
        authority: AuthorityId,
        context: ContextId,
        device_id: DeviceId,
    ) -> RuntimeLaunchSpec<'_> {
        RuntimeLaunchSpec {
            launch: self,
            authority,
            context,
            device_id,
        }
    }
}

#[derive(Debug, Clone)]
struct RuntimeLaunchSpec<'a> {
    launch: &'a ResolvedTuiLaunch,
    authority: AuthorityId,
    context: ContextId,
    device_id: DeviceId,
}

impl RuntimeLaunchSpec<'_> {
    fn agent_config(&self) -> Result<AgentConfig, AuraError> {
        let mut config = AgentConfig {
            device_id: self.device_id,
            storage: StorageConfig {
                base_path: self.launch.base_path.clone(),
                ..StorageConfig::default()
            },
            network: NetworkConfig {
                bind_address: self
                    .launch
                    .bind_address
                    .as_deref()
                    .unwrap_or("0.0.0.0:0")
                    .to_string(),
                ..NetworkConfig::default()
            },
            ..AgentConfig::default()
        };
        if let Some(backend) = crate::env::secure_storage_backend_override()? {
            config.storage.secure_storage_backend = backend;
        }
        harness_lan_discovery_override(&mut config);
        bootstrap_broker_override(&mut config);
        Ok(config)
    }

    /// The production runtime of this launch, as the TUI runs it, on the
    /// profile `profile` owns (its account records share the owner).
    async fn build_production_agent(&self, profile: &ProfileStore) -> Result<AuraAgent, AuraError> {
        let config = self.agent_config()?;
        let builder = match profile.owner() {
            Some(owner) => AgentBuilder::new().with_profile_owner(owner),
            None => AgentBuilder::new(),
        };
        builder
            .with_config(config.clone())
            .with_authority(self.authority)
            .with_sync_config(self.sync_config())
            .with_rendezvous_config(config.rendezvous_config())
            .build_production(&self.effect_context())
            .await
            .map_err(|error| AuraError::Internal {
                message: "Failed to create agent".into(),
                source: Some(std::sync::Arc::new(error)),
            })
    }

    fn effect_context(&self) -> EffectContext {
        EffectContext::new(
            self.authority,
            self.context,
            self.launch.mode.execution_mode(),
        )
    }

    fn sync_config(&self) -> SyncManagerConfig {
        SyncManagerConfig {
            auto_sync_interval: Duration::from_secs(2),
            ..SyncManagerConfig::default()
        }
    }
}

/// Resolve the storage base path for Aura.
pub fn resolve_storage_path(explicit_override: Option<&str>, mode: TuiMode) -> PathBuf {
    if let Some(path) = explicit_override {
        return PathBuf::from(path);
    }

    let aura_path = aura_agent::core::config::aura_storage_root();

    match mode {
        TuiMode::Production => aura_path.join(".aura"),
        TuiMode::Demo { .. } => aura_path.join(".aura-demo"),
    }
}

fn harness_lan_discovery_override(current: &mut aura_agent::core::config::AgentConfig) {
    let Some(override_env) = harness_lan_discovery_env_override() else {
        return;
    };

    current.lan_discovery.enabled = override_env.enabled;
    current.lan_discovery.bind_addr = override_env.bind_addr;
    current.lan_discovery.broadcast_addr = override_env.broadcast_addr;
    current.lan_discovery.port = override_env.port.unwrap_or(current.lan_discovery.port);

    tracing::info!(
        enabled = current.lan_discovery.enabled,
        bind_addr = %current.lan_discovery.bind_addr,
        broadcast_addr = %current.lan_discovery.broadcast_addr,
        port = current.lan_discovery.port,
        "Applied harness LAN discovery overrides from environment"
    );
}

fn bootstrap_broker_override(current: &mut aura_agent::core::config::AgentConfig) {
    let Some(override_env) = bootstrap_broker_env_override() else {
        return;
    };

    let mut broker = BootstrapBrokerConfig::default().enabled(true);
    if let Some(bind_addr) = override_env.bind_addr {
        broker = broker.with_bind_addr(bind_addr);
    }
    broker = broker.with_lan_bind_policy(override_env.lan_bind_policy);
    if let Some(base_url) = override_env.base_url {
        broker = broker.with_base_url(base_url);
    }
    if let Some(auth_token) = override_env.auth_token {
        broker = broker.with_auth_token(auth_token);
    }
    if let Some(invitation_retrieval_token) = override_env.invitation_retrieval_token {
        broker = broker.with_invitation_retrieval_token(invitation_retrieval_token);
    }
    current.bootstrap_broker = broker;

    tracing::info!(
        bind_addr = ?current.bootstrap_broker.bind_addr,
        base_url = ?current.bootstrap_broker.base_url,
        lan_bind_policy = ?current.bootstrap_broker.lan_bind_policy,
        "Applied bootstrap broker overrides from environment"
    );
}

fn panic_payload_message(payload: Box<dyn std::any::Any + Send>) -> String {
    if let Some(message) = payload.downcast_ref::<&'static str>() {
        (*message).to_string()
    } else if let Some(message) = payload.downcast_ref::<String>() {
        message.clone()
    } else {
        "non-string panic payload".to_string()
    }
}

fn reexec_current_tui_process(reason: &str) -> Result<(), AuraError> {
    let current_exe = env::current_exe().map_err(|error| {
        AuraError::internal(format!(
            "Failed to resolve current TUI executable for {reason}: {error}"
        ))
    })?;
    let error = Command::new(current_exe)
        .args(env::args_os().skip(1))
        .exec();
    Err(AuraError::internal(format!(
        "Failed to re-exec TUI process for {reason}: {error}"
    )))
}

async fn initialized_runtime_app_core(
    app_config: AppConfig,
    agent: Arc<AuraAgent>,
    bootstrap_app: Option<Arc<RwLock<AppCore>>>,
) -> Result<InitializedAppCore, AuraError> {
    if let Some(app_core) = bootstrap_app {
        app_core
            .write()
            .await
            .attach_bootstrap_runtime(agent.as_runtime_bridge())
            .map_err(|error| AuraError::Internal {
                message: "attach bootstrap runtime".into(),
                source: Some(Arc::new(error)),
            })?;
        return InitializedAppCore::new(app_core).await;
    }
    let app_core = AppCore::with_runtime(app_config, agent.as_runtime_bridge())
        .map_err(|error| AuraError::internal(format!("Failed to create AppCore: {error}")))?;
    let app_core = Arc::new(RwLock::new(app_core));
    InitializedAppCore::new(app_core).await
}

#[cfg(feature = "development")]
fn build_demo_io_context(
    app_core: InitializedAppCore,
    launch: &ResolvedTuiLaunch,
    device_label: &str,
    has_existing_account: bool,
    pending_runtime_bootstrap: bool,
    profile: &ProfileStore,
    simulator: Option<&DemoSimulator>,
    signed_codes: Option<(String, String)>,
) -> crate::error::TerminalResult<IoContext> {
    let seed = match launch.mode {
        TuiMode::Demo { seed } => seed,
        TuiMode::Production => {
            return Err(crate::error::TerminalError::Config(
                "demo context builder cannot be used for production launches".to_string(),
            ));
        }
    };
    let mut hints = DemoHints::new(seed);
    if let Some((alice, carol)) = signed_codes {
        hints.alice_invite_code = alice;
        hints.carol_invite_code = carol;
    }
    let mut builder = IoContext::builder()
        .with_app_core(app_core)
        .with_base_path(launch.base_path.clone())
        .with_profile_store(profile.clone())
        .with_device_id(device_label.to_string())
        .with_mode(launch.mode)
        .with_existing_account(has_existing_account)
        .with_pending_runtime_bootstrap(pending_runtime_bootstrap)
        .with_demo_hints(hints);

    if let Some(sim) = simulator {
        builder = builder
            .with_demo_mobile_agent(sim.mobile_agent())
            .with_demo_mobile_device_id(sim.mobile_device_id().to_string())
            .with_demo_mobile_authority_id(sim.mobile_authority().to_string());
    }

    builder.build().map_err(|error| {
        crate::error::TerminalError::Config(format!("IoContext build failed: {error}"))
    })
}

fn build_standard_io_context(
    app_core: InitializedAppCore,
    launch: &ResolvedTuiLaunch,
    device_label: &str,
    has_existing_account: bool,
    pending_runtime_bootstrap: bool,
    profile: &ProfileStore,
) -> crate::error::TerminalResult<IoContext> {
    IoContext::builder()
        .with_app_core(app_core)
        .with_base_path(launch.base_path.clone())
        .with_profile_store(profile.clone())
        .with_device_id(device_label.to_string())
        .with_mode(launch.mode)
        .with_existing_account(has_existing_account)
        .with_pending_runtime_bootstrap(pending_runtime_bootstrap)
        .build()
        .map_err(|error| {
            crate::error::TerminalError::Config(format!("IoContext build failed: {error}"))
        })
}

/// Serve `aura rpc` sessions on the account's node socket for as long as
/// the TUI's startup task owner lives. A socket already answered by another
/// node is left alone.
async fn host_node_socket(
    tasks: &UiTaskOwner,
    base_path: &std::path::Path,
    ctx: crate::command::CommandContext,
) {
    let socket = crate::rpc_socket::socket_path(base_path);
    match crate::rpc_socket::bind(&socket).await {
        Ok(listener) => tasks.spawn_cancellable(async move {
            if let Err(error) =
                crate::rpc_socket::serve(&ctx, listener, &socket, std::future::pending()).await
            {
                tracing::warn!(%error, "node socket stopped");
            }
        }),
        Err(error) => tracing::warn!(%error, "node socket not hosted"),
    }
}

/// Reconcile a staged account bootstrap into the runtime, as the first
/// launch on a newly created account does: the shared reconciler initializes
/// the runtime account (and Note to Self), then the staged record is
/// cleared. A pending device enrollment is left staged; its code is returned.
async fn reconcile_pending_bootstrap(
    storage: &impl aura_core::effects::StorageExtendedEffects,
    app_core: &Arc<RwLock<AppCore>>,
    pending_bootstrap: aura_app::ui::types::PendingAccountBootstrap,
    base_path: &std::path::Path,
    surface: BootstrapSurface,
) -> Result<Option<String>, AuraError> {
    use aura_app::ui::workflows::account::{
        reconcile_pending_runtime_account_bootstrap, PendingRuntimeBootstrapAction,
    };
    let enrollment_code = pending_bootstrap.device_enrollment_code.clone();
    let resolution =
        reconcile_pending_runtime_account_bootstrap(app_core, Some(pending_bootstrap)).await?;
    let clear = matches!(
        resolution.action,
        PendingRuntimeBootstrapAction::ClearedStalePending
    ) || matches!(
        resolution.action,
        PendingRuntimeBootstrapAction::InitializedFromPending
    ) && enrollment_code.is_none();
    if clear {
        let event = BootstrapEvent::new(surface, BootstrapEventKind::PendingBootstrapReconciled);
        tracing::info!(event = %event, path = %base_path.display());
        clear_pending_account_bootstrap(storage).await?;
    }
    if resolution.account_ready && enrollment_code.is_none() {
        let event = BootstrapEvent::new(surface, BootstrapEventKind::RuntimeBootstrapFinalized);
        tracing::info!(event = %event, path = %base_path.display());
    }
    Ok(enrollment_code)
}

/// The account's production runtime, opened the way the TUI opens it.
pub struct ProductionRuntime {
    pub agent: Arc<AuraAgent>,
    /// App core on the runtime, signals initialized, staged account bootstrap
    /// reconciled.
    pub app_core: Arc<RwLock<AppCore>>,
    pub authority: AuthorityId,
    pub context: ContextId,
}

/// Open the production runtime of the account stored at `base_path`, under
/// the profile's exclusive lease (fails while another process holds it), and
/// finish a newly created account's bootstrap the way the TUI's first launch
/// does. Returns `Ok(None)` when no account exists there.
pub async fn open_production_runtime(
    base_path: &std::path::Path,
) -> Result<Option<ProductionRuntime>, AuraError> {
    let launch = TuiLaunchRequest {
        data_dir: base_path.to_str(),
        requested_device_id: None,
        bind_address: None,
        mode: TuiMode::Production,
    }
    .resolve();
    let profile = ProfileStore::production(&launch.base_path)?;
    let storage = profile.storage().clone();
    let AccountLoadResult::Loaded {
        authority, context, ..
    } = try_load_account(&storage).await?
    else {
        return Ok(None);
    };
    let device_id = load_selected_runtime_identity(&storage)
        .await?
        .map(|identity| identity.device_id)
        .unwrap_or(launch.configured_device_id);
    if load_pending_account_bootstrap(&storage)
        .await?
        .is_some_and(|pending| pending.has_pending_device_enrollment())
    {
        return Err(AuraError::invalid(
            "this account has a pending device enrollment; finish it in the TUI",
        ));
    }
    let agent = Arc::new(
        launch
            .runtime_spec(authority, context, device_id)
            .build_production_agent(&profile)
            .await?,
    );
    let app_core = initialized_runtime_app_core(launch.app_config(), agent.clone(), None)
        .await?
        .raw()
        .clone();
    if let Some(pending) = load_pending_account_bootstrap(&storage).await? {
        reconcile_pending_bootstrap(
            &storage,
            &app_core,
            pending,
            &launch.base_path,
            BootstrapSurface::Terminal,
        )
        .await?;
    }
    Ok(Some(ProductionRuntime {
        agent,
        app_core,
        authority,
        context,
    }))
}

/// Handle TUI launch.
pub async fn handle_tui(args: &TuiArgs) -> crate::error::TerminalResult<()> {
    let stdio = PreFullscreenStdio::new();
    let launch = TuiLaunchRequest::from_args(&stdio, args).resolve();
    handle_tui_launch(stdio, launch).await
}

async fn handle_tui_launch(
    stdio: PreFullscreenStdio,
    launch: ResolvedTuiLaunch,
) -> crate::error::TerminalResult<()> {
    handle_tui_launch_with_bootstrap(stdio, launch, None, None).await
}

/// `retained_profile`: the in-process bootstrap generation keeps the profile
/// (and its exclusive owner) the first generation acquired.
async fn handle_tui_launch_with_bootstrap(
    mut stdio: PreFullscreenStdio,
    launch: ResolvedTuiLaunch,
    bootstrap_app: Option<Arc<RwLock<AppCore>>>,
    retained_profile: Option<ProfileStore>,
) -> crate::error::TerminalResult<()> {
    launch.print_startup(&mut stdio);

    let profile = match retained_profile {
        Some(profile) => profile,
        None => ProfileStore::for_mode(&launch.base_path, launch.mode)?,
    };
    let storage = profile.storage().clone();
    if launch.mode.is_demo() {
        cleanup_demo_storage(&storage, &launch.base_path).await;
    }

    // The in-process bootstrap generation retains the original global logger
    // and its owned storage writer, rather than allocating another writer.
    if bootstrap_app.is_none() {
        init_tui_tracing(storage.clone(), launch.mode);
    }
    std::env::set_var(
        "AURA_DEMO_BOB_DEVICE_ID",
        launch.configured_device_id.to_string(),
    );

    let app_config = launch.app_config();
    let selected_runtime_identity = load_selected_runtime_identity(&storage).await?;
    if load_pending_account_bootstrap(&storage)
        .await?
        .is_some_and(|pending| pending.has_pending_device_enrollment())
    {
        return Err(AuraError::PermissionDenied {
            message:
                "Legacy pending enrollment requires independently transferred manifest and verifier"
                    .into(),
            source: Some(Arc::new(
                aura_app::ui::workflows::ceremonies::EnrollmentManifestError::MissingPin,
            )),
        }
        .into());
    }

    let device_id = selected_runtime_identity
        .as_ref()
        .map(|identity| identity.device_id)
        .unwrap_or(launch.configured_device_id);
    let device_label = device_id.to_string();
    let loaded_account = try_load_account(&storage).await?;
    let has_existing_account = matches!(loaded_account, AccountLoadResult::Loaded { .. });

    #[cfg(feature = "development")]
    let mut simulator: Option<DemoSimulator> = None;
    let mut pending_runtime_bootstrap = false;
    let startup_tasks = Arc::new(UiTaskOwner::new());

    let app_core = match loaded_account {
        AccountLoadResult::Loaded {
            authority,
            context,
            nickname_suggestion,
        } => {
            let runtime_spec = launch.runtime_spec(authority, context, device_id);
            stdio.println(format_args!("Authority: {authority}"));
            stdio.println(format_args!("Context: {context}"));

            #[cfg(feature = "development")]
            let demo_simulator_for_bob = match launch.mode {
                TuiMode::Demo { seed } => {
                    stdio.println(format_args!(
                        "Creating demo simulator for shared transport..."
                    ));
                    let simulator =
                        DemoSimulator::new(seed, launch.base_path.clone(), authority, context)
                            .await
                            .map_err(|error| {
                                AuraError::internal(format!("Failed to create simulator: {error}"))
                            })?;
                    Some(simulator)
                }
                TuiMode::Production => None,
            };

            let agent = match launch.mode {
                TuiMode::Production => runtime_spec.build_production_agent(&profile).await?,
                TuiMode::Demo { seed } => {
                    stdio.println(format_args!("Using simulation agent with seed: {seed}"));

                    #[cfg(feature = "development")]
                    {
                        let shared_transport = demo_simulator_for_bob
                            .as_ref()
                            .map(DemoSimulator::shared_transport)
                            .ok_or_else(|| {
                                AuraError::internal("Simulator not available in demo mode")
                            })?;

                        stdio.println(format_args!(
                            "Creating Bob's agent with shared transport..."
                        ));
                        AgentBuilder::new()
                                .with_config(runtime_spec.agent_config()?)
                                .with_authority(authority)
                                .with_sync_config(runtime_spec.sync_config())
                                .with_rendezvous_config(
                                    runtime_spec.agent_config()?.rendezvous_config(),
                                )
                                .build_simulation_async_with_shared_transport(
                                    seed,
                                    &runtime_spec.effect_context(),
                                    shared_transport,
                                )
                                .await
                                .map_err(|error| {
                                    AuraError::internal(format!(
                                        "Failed to create simulation agent with shared transport: {error}"
                                    ))
                                })?
                    }

                    #[cfg(not(feature = "development"))]
                    {
                        AgentBuilder::new()
                            .with_config(runtime_spec.agent_config()?)
                            .with_authority(authority)
                            .with_sync_config(runtime_spec.sync_config())
                            .build_simulation_async(seed, &runtime_spec.effect_context())
                            .await
                            .map_err(|error| {
                                AuraError::internal(format!(
                                    "Failed to create simulation agent: {error}"
                                ))
                            })?
                    }
                }
            };

            let agent = Arc::new(agent);
            let app_core =
                initialized_runtime_app_core(app_config, agent.clone(), bootstrap_app).await?;
            let mut pending_device_enrollment_code = None;

            let pending_bootstrap = load_pending_account_bootstrap(&storage).await?;
            if let Some(pending_bootstrap) = pending_bootstrap {
                pending_device_enrollment_code = reconcile_pending_bootstrap(
                    &storage,
                    app_core.raw(),
                    pending_bootstrap,
                    &launch.base_path,
                    BootstrapSurface::Tui,
                )
                .await?;
                pending_runtime_bootstrap = pending_device_enrollment_code.is_some();
            } else if context_workflows::current_home_context(app_core.raw())
                .await
                .is_err()
            {
                let nickname_suggestion = nickname_suggestion.clone().ok_or_else(|| {
                    AuraError::internal(
                        "Loaded account is missing bootstrap nickname for runtime initialization"
                            .to_string(),
                    )
                })?;
                if let Err(error) =
                    initialize_runtime_account(app_core.raw(), nickname_suggestion).await
                {
                    return Err(error.into());
                }
                let finalized_event = BootstrapEvent::new(
                    BootstrapSurface::Tui,
                    BootstrapEventKind::RuntimeBootstrapFinalized,
                );
                tracing::info!(
                    event = %finalized_event,
                    path = %launch.base_path.display()
                );
            }

            if let Err(error) =
                aura_app::ui::workflows::settings::refresh_settings_from_runtime(app_core.raw())
                    .await
            {
                stdio.eprintln(format_args!("Warning: Failed to refresh settings: {error}"));
            }

            stdio.println(format_args!(
                "AppCore initialized (with runtime bridge and reactive signals)"
            ));

            // Host the node socket so `aura ...` commands reach this runtime
            // while the TUI holds the profile.
            if matches!(launch.mode, TuiMode::Production) {
                host_node_socket(
                    &startup_tasks,
                    &launch.base_path,
                    crate::command::CommandContext::new(
                        app_core.raw().clone(),
                        agent.runtime().effects(),
                        authority,
                    ),
                )
                .await;
            }

            if let Some(device_enrollment_code) = pending_device_enrollment_code {
                let startup_core = app_core.raw().clone();
                let startup_storage = storage.clone();
                let startup_path = launch.base_path.clone();
                startup_tasks.spawn(async move {
                        match aura_app::ui::workflows::invitation::import_invitation_details(
                            &startup_core,
                            &device_enrollment_code,
                        )
                        .await
                        {
                            Ok(invitation) => {
                                match aura_app::ui::workflows::invitation::accept_device_enrollment_invitation(
                                    &startup_core,
                                    invitation.info(),
                                )
                                .await
                                {
                                    Ok(()) => {
                                        let reconciled_event = BootstrapEvent::new(
                                            BootstrapSurface::Tui,
                                            BootstrapEventKind::PendingBootstrapReconciled,
                                        );
                                        tracing::info!(
                                            event = %reconciled_event,
                                            path = %startup_path.display()
                                        );
                                        if let Err(error) =
                                            clear_pending_account_bootstrap(&startup_storage)
                                                .await
                                        {
                                            tracing::error!(
                                                path = %startup_path.display(),
                                                error = %error,
                                                "failed to clear pending account bootstrap after device enrollment acceptance"
                                            );
                                            return;
                                        }
                                        let finalized_event = BootstrapEvent::new(
                                            BootstrapSurface::Tui,
                                            BootstrapEventKind::RuntimeBootstrapFinalized,
                                        );
                                        tracing::info!(
                                            event = %finalized_event,
                                            path = %startup_path.display()
                                        );
                                    }
                                    Err(error) => {
                                        tracing::error!(
                                            path = %startup_path.display(),
                                            error = %error,
                                            "failed to accept pending startup device enrollment invitation"
                                        );
                                    }
                                }
                            }
                            Err(error) => {
                                tracing::error!(
                                    path = %startup_path.display(),
                                    error = %error,
                                    "failed to import pending startup device enrollment invitation"
                                );
                            }
                        }
                    });
            }

            #[cfg(feature = "development")]
            {
                simulator = match launch.mode {
                    TuiMode::Demo { .. } => {
                        stdio.println(format_args!("Starting demo simulator..."));
                        let mut sim = demo_simulator_for_bob.ok_or_else(|| {
                            AuraError::internal("Simulator not available in demo mode")
                        })?;
                        sim.enable_realistic_world(launch.base_path.clone())
                            .await
                            .map_err(|error| {
                                AuraError::internal(format!(
                                    "Failed to build realistic demo world: {error}"
                                ))
                            })?;
                        sim.start().await.map_err(|error| {
                            AuraError::internal(format!("Failed to start simulator: {error}"))
                        })?;

                        stdio.println(format_args!("Alice online: {}", sim.alice_authority()));
                        stdio.println(format_args!("Carol online: {}", sim.carol_authority()));
                        stdio.println(format_args!("Mobile online: {}", sim.mobile_authority()));
                        std::env::set_var(
                            "AURA_DEMO_DEVICE_ID",
                            sim.mobile_device_id().to_string(),
                        );

                        seed_realistic_demo_world(app_core.raw(), &agent, &sim)
                            .await
                            .map_err(|error| {
                                AuraError::internal(format!("Failed to seed demo world: {error}"))
                            })?;

                        if let Err(error) =
                            aura_app::ui::workflows::system::refresh_account(app_core.raw()).await
                        {
                            tracing::warn!("Demo: Failed to refresh account state: {}", error);
                        }

                        Some(sim)
                    }
                    TuiMode::Production => None,
                };
            }

            #[cfg(feature = "development")]
            if let TuiMode::Demo { .. } = launch.mode {
                if let Some(sim) = &simulator {
                    let effects = agent.runtime().effects();
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
                    let _amp_inbox_handle = spawn_amp_inbox_listener(effects, authority, peers);
                }
            }

            app_core
        }
        AccountLoadResult::NotFound => {
            if let Some(runtime_authority) = selected_runtime_identity
                .as_ref()
                .map(|identity| identity.authority_id)
            {
                let context = default_context_id_for_authority(runtime_authority);
                let runtime_spec = launch.runtime_spec(runtime_authority, context, device_id);
                stdio.println(format_args!(
                        "No existing account - binding provisional runtime authority {runtime_authority}"
                    ));

                let agent = match launch.mode {
                    TuiMode::Production => runtime_spec.build_production_agent(&profile).await?,
                    TuiMode::Demo { seed } => AgentBuilder::new()
                        .with_config(runtime_spec.agent_config()?)
                        .with_authority(runtime_authority)
                        .with_sync_config(runtime_spec.sync_config())
                        .with_rendezvous_config(runtime_spec.agent_config()?.rendezvous_config())
                        .build_simulation_async(seed, &runtime_spec.effect_context())
                        .await
                        .map_err(|error| {
                            AuraError::internal(format!(
                                "Failed to create provisional simulation agent: {error}"
                            ))
                        })?,
                };

                let agent = Arc::new(agent);
                let app_core =
                    initialized_runtime_app_core(app_config, agent.clone(), None).await?;

                if let Err(error) =
                    aura_app::ui::workflows::settings::refresh_settings_from_runtime(app_core.raw())
                        .await
                {
                    stdio.eprintln(format_args!(
                        "Warning: Failed to refresh provisional settings: {error}"
                    ));
                }

                stdio.println(format_args!(
                    "AppCore initialized (bootstrap shell with provisional runtime)"
                ));
                app_core
            } else {
                let waiting_event = BootstrapEvent::new(
                    BootstrapSurface::Tui,
                    BootstrapEventKind::ShellAwaitingAccount,
                );
                tracing::info!(event = %waiting_event, path = %launch.base_path.display());
                stdio.println(format_args!("No existing account - will show setup modal"));
                let app_core =
                    Arc::new(RwLock::new(AppCore::new(app_config).map_err(|error| {
                        AuraError::internal(format!("Failed to create AppCore: {error}"))
                    })?));
                let app_core = InitializedAppCore::new(app_core).await?;
                stdio.println(format_args!(
                    "AppCore initialized (bootstrap shell without runtime)"
                ));
                app_core
            }
        }
    };

    let retained_bootstrap_app = app_core.raw().clone();
    let bootstrap_has_runtime = app_core.runtime().is_some();
    #[cfg(feature = "development")]
    let ctx = match launch.mode {
        TuiMode::Demo { .. } => {
            // Hint codes must be real signed invitations or they cannot import.
            let signed_codes = match simulator.as_ref() {
                Some(sim) => match sim.signed_contact_invite_codes().await {
                    Ok(codes) => Some(codes),
                    Err(error) => {
                        tracing::warn!(error = %error, "demo: could not create signed contact codes");
                        None
                    }
                },
                None => None,
            };
            let ctx = build_demo_io_context(
                app_core,
                &launch,
                &device_label,
                has_existing_account,
                pending_runtime_bootstrap,
                simulator.as_ref(),
                signed_codes,
            )?;
            stdio.println(format_args!("Demo hints available."));
            ctx
        }
        TuiMode::Production => build_standard_io_context(
            app_core,
            &launch,
            &device_label,
            has_existing_account,
            pending_runtime_bootstrap,
            &profile,
        )?,
    };

    #[cfg(not(feature = "development"))]
    let ctx = build_standard_io_context(
        app_core,
        &launch,
        &device_label,
        has_existing_account,
        pending_runtime_bootstrap,
        &profile,
    )?;

    #[cfg(not(feature = "development"))]
    if launch.mode.is_demo() {
        stdio.println(format_args!(
            "Note: Demo mode simulation requires the 'development' feature."
        ));
        stdio.println(format_args!(
            "Running with simulation agent but without peer agents (Alice/Carol)."
        ));
    }

    stdio.println(format_args!("Launching TUI..."));
    stdio.newline();

    let (returned_stdio, result) = during_fullscreen(
        stdio,
        AssertUnwindSafe(run_app_with_context(ctx)).catch_unwind(),
    )
    .await;
    #[cfg(feature = "development")]
    {
        stdio = returned_stdio.into();
    }
    #[cfg(not(feature = "development"))]
    let stdio: PreFullscreenStdio = returned_stdio.into();
    let shell_exit_intent = match result {
        Ok(Ok(intent)) => intent,
        Ok(Err(source)) => {
            return Err(AuraError::Internal {
                message: "TUI fullscreen generation failed".into(),
                source: Some(Arc::new(source)),
            }
            .into());
        }
        Err(payload) => {
            return Err(AuraError::internal(format!(
                "TUI fullscreen generation panicked: {}",
                panic_payload_message(payload)
            ))
            .into())
        }
    };

    #[cfg(feature = "development")]
    if let Some(ref mut sim) = simulator {
        stdio.println(format_args!("Stopping demo simulator..."));
        if let Err(error) = sim.stop().await {
            stdio.eprintln(format_args!(
                "Warning: Failed to stop simulator cleanly: {error}"
            ));
        }
    }

    match shell_exit_intent {
        ShellExitIntent::UserQuit => {}
        ShellExitIntent::BootstrapReload => {
            if !launch
                .base_path
                .join(".bootstrap-runtime-handoff-ready")
                .exists()
            {
                return Err(AuraError::internal(
                    "bootstrap reload requested without persisted handoff marker",
                )
                .into());
            }
            match wait_for_persisted_account(
                &storage,
                std::time::Duration::from_secs(5),
                std::time::Duration::from_millis(50),
            )
            .await?
            {
                AccountLoadResult::Loaded { .. } => {}
                AccountLoadResult::NotFound => {
                    return Err(AuraError::internal(
                            "bootstrap runtime handoff marker was set before persisted account became observable",
                        )
                        .into());
                }
            }
            // Suppress visible output — the re-exec enters fullscreen immediately
            // and any stdout here would flash on the normal terminal buffer.
            tracing::info!("Reloading TUI with newly created bootstrap identity");
            if !bootstrap_has_runtime {
                // This runtime-free generation never admits enrollment startup
                // work. Release its task root before entering another shell.
                startup_tasks.shutdown();
                drop(startup_tasks);
                return Box::pin(handle_tui_launch_with_bootstrap(
                    stdio,
                    launch,
                    Some(retained_bootstrap_app),
                    Some(profile),
                ))
                .await;
            }
            return reexec_current_tui_process("bootstrap reload").map_err(Into::into);
        }
        ShellExitIntent::AuthoritySwitch {
            authority_id,
            nickname_suggestion,
        } => {
            let _ = persist_selected_authority(&profile, authority_id, nickname_suggestion).await?;
            tracing::info!("Reloading TUI for authority: {authority_id}");
            return reexec_current_tui_process("authority switch").map_err(Into::into);
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    #[tokio::test]
    async fn production_runtime_opens_a_newly_created_account() {
        let dir = tempfile::tempdir().unwrap();
        super::create_account(dir.path(), "Alex").await.unwrap();
        let runtime = super::open_production_runtime(dir.path())
            .await
            .unwrap()
            .expect("the created account is found");
        aura_app::ui::workflows::settings::refresh_settings_from_runtime(&runtime.app_core)
            .await
            .unwrap();
        let settings = aura_app::ui::workflows::settings::get_settings(&runtime.app_core)
            .await
            .unwrap();
        assert_eq!(settings.nickname_suggestion, "Alex");
    }

    #[test]
    fn demo_runtime_paths_keep_rendezvous_enabled() {
        let source = include_str!("mod.rs");

        assert!(
            source.contains(
                ".with_sync_config(runtime_spec.sync_config())\n                                .with_rendezvous_config(\n                                    runtime_spec.agent_config()?.rendezvous_config(),\n                                )\n                                .build_simulation_async_with_shared_transport("
            ),
            "demo shared-transport runtime must enable rendezvous so bootstrap discovery stays live"
        );
        assert!(
            source.contains(
                ".with_sync_config(runtime_spec.sync_config())\n                        .with_rendezvous_config(runtime_spec.agent_config()?.rendezvous_config())\n                        .build_simulation_async(seed, &runtime_spec.effect_context())"
            ),
            "demo provisional runtime must enable rendezvous so bootstrap discovery starts before enrollment"
        );
    }
}
