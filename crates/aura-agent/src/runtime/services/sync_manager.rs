//! Sync Service Manager
//!
//! Wraps `aura_sync::SyncService` for integration with the agent runtime.
//! Provides lifecycle management and configuration for automatic background sync.

use super::config_profiles::impl_service_config_profiles;
use super::service_actor::{
    validate_actor_transition, ActorLifecyclePhase, ActorOwnedServiceRoot, ServiceActorHandle,
};
use super::traits::{
    RuntimeService, RuntimeServiceContext, ServiceError, ServiceErrorKind, ServiceHealth,
};
use super::{ReconfigurationManager, ReconfigurationManagerError, SessionDelegationTransfer};
use crate::core::default_context_id_for_authority;
use crate::runtime::vm_host_bridge::{AuraVmHostWaitStatus, AuraVmRoundDisposition};
use crate::runtime::{
    open_owned_manifest_vm_session_admitted, AuraEffectSystem, RuntimeChoreographySessionId,
    TaskGroup,
};
use async_trait::async_trait;
#[cfg(test)]
use aura_core::effects::indexed::IndexedFact;
use aura_core::effects::indexed::IndexedJournalEffects;
use aura_core::effects::PhysicalTimeEffects;
use aura_core::hash::hash;
use aura_core::util::serialization::to_vec;
use aura_core::{AuthorityId, DelegationReceipt, DeviceId, OwnershipCategory};
use aura_protocol::effects::{ChoreographicRole, RoleIndex};
use aura_sync::protocols::epoch_runners::EpochRotationProtocolRole;
use aura_sync::protocols::{EpochCommit, EpochConfirmation, EpochRotationProposal};
use aura_sync::services::{Service, SyncService, SyncServiceConfig};
use aura_sync::verification::MerkleVerifier;
#[cfg(test)]
use aura_sync::verification::VerificationResult;
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use std::time::Duration;
use telltale_machine::StepResult;
use thiserror::Error;
use tokio::sync::{oneshot, Mutex};
use uuid::Uuid;

/// Configuration for the sync service manager
#[derive(Debug, Clone)]
pub struct SyncManagerConfig {
    /// Enable automatic periodic sync
    pub auto_sync_enabled: bool,

    /// Interval between automatic sync rounds (default: 60s)
    pub auto_sync_interval: Duration,

    /// Maximum concurrent sync sessions
    pub max_concurrent_syncs: usize,

    /// Actual journal/anti-entropy resource and retry policy. This cannot extend
    /// the admitted caller's original operation deadline.
    pub journal_sync: aura_sync::protocols::journal::JournalSyncConfig,

    /// Initial peers to sync with (can be empty if using discovery)
    pub initial_peers: Vec<DeviceId>,

    /// Enable periodic maintenance cleanup
    pub maintenance_enabled: bool,

    /// Interval between maintenance runs
    pub maintenance_interval: Duration,

    /// TTL for stale peer states
    pub peer_state_ttl: Duration,

    /// Maximum tracked peer states before pruning
    pub max_peer_states: usize,
}

impl Default for SyncManagerConfig {
    fn default() -> Self {
        Self {
            auto_sync_enabled: true,
            auto_sync_interval: Duration::from_secs(60),
            max_concurrent_syncs: 5,
            initial_peers: Vec::new(),
            journal_sync: Default::default(),
            maintenance_enabled: true,
            maintenance_interval: Duration::from_secs(60),
            peer_state_ttl: Duration::from_secs(6 * 60 * 60),
            max_peer_states: 1024,
        }
    }
}

impl_service_config_profiles!(SyncManagerConfig {
    /// Create config for testing (shorter intervals)
    pub fn for_testing() -> Self {
        Self {
            auto_sync_enabled: true,
            auto_sync_interval: Duration::from_secs(5),
            max_concurrent_syncs: 3,
            initial_peers: Vec::new(),
            journal_sync: Default::default(),
            maintenance_enabled: true,
            maintenance_interval: Duration::from_secs(5),
            peer_state_ttl: Duration::from_secs(60),
            max_peer_states: 128,
        }
    }

    /// Create config with auto-sync disabled
    pub fn manual_only() -> Self {
        Self {
            auto_sync_enabled: false,
            auto_sync_interval: Duration::from_secs(60),
            max_concurrent_syncs: 5,
            initial_peers: Vec::new(),
            journal_sync: Default::default(),
            maintenance_enabled: true,
            maintenance_interval: Duration::from_secs(60),
            peer_state_ttl: Duration::from_secs(6 * 60 * 60),
            max_peer_states: 1024,
        }
    }
});

/// State of the sync service manager
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncManagerState {
    /// Service not yet started
    Stopped,
    /// Service starting up
    Starting,
    /// Service running and actively syncing
    Running,
    /// Service shutting down
    Stopping,
    /// Service hit an observable lifecycle failure.
    Failed,
}

#[derive(Debug, Error)]
pub enum SyncManagerError {
    #[error(transparent)]
    Service(#[from] ServiceError),
    #[error("sync service not started")]
    NotStarted,
    #[error("sync operation failed: {0}")]
    Sync(String),
    #[error("sync operation failed: {0}")]
    NativeSync(#[source] aura_core::AuraError),
    #[error("required sync physical observation failed")]
    RequiredClock(#[source] aura_core::effects::time::TimeError),
    #[error("journal sync backing off after authenticated authorization denial")]
    AuthorizationBackoff,
    #[error("epoch rotation delegation requires bundle evidence")]
    DelegationRequiresBundleEvidence,
    #[error("epoch rotation delegation failed")]
    Delegation(#[source] ReconfigurationManagerError),
    #[error("epoch rotation participant role required")]
    ParticipantRoleRequired,
    #[error("epoch rotation VM session open failed: {0}")]
    VmSessionOpen(String),
    #[error("epoch rotation VM advance failed: {0}")]
    VmAdvance(String),
    #[error("epoch rotation VM round handling failed: {0}")]
    VmRoundHandling(String),
    #[error("epoch proposal encode failed: {0}")]
    ProposalEncode(String),
    #[error("epoch confirmation encode failed: {0}")]
    ConfirmationEncode(String),
    #[error("epoch confirmation decode failed: {0}")]
    ConfirmationDecode(String),
    #[error("epoch commit encode failed: {0}")]
    CommitEncode(String),
    #[error("epoch rotation {role} VM timed out while waiting for receive")]
    VmTimedOut { role: &'static str },
    #[error("epoch rotation {role} VM cancelled while waiting for receive")]
    VmCancelled { role: &'static str },
    #[error("epoch rotation {role} VM became stuck without a pending receive")]
    VmStuck { role: &'static str },
}

struct SyncState {
    service: Arc<SyncService>,
    peers: Vec<DeviceId>,
}

/// Minimum spacing between syncs with one peer. Many workflows trigger sync;
/// repeated triggers inside this window are coalesced into the last attempt.
const MIN_PEER_RESYNC_INTERVAL_MS: u64 = 3_000;

/// Peers not attempted within the resync window, recording `now_ms` for each.
fn take_due_peers(
    recent: &mut HashMap<DeviceId, u64>,
    peers: Vec<DeviceId>,
    now_ms: u64,
) -> Vec<DeviceId> {
    peers
        .into_iter()
        .filter(|peer| {
            let due = recent.get(peer).map_or(true, |last| {
                now_ms.saturating_sub(*last) >= MIN_PEER_RESYNC_INTERVAL_MS
            });
            if due {
                recent.insert(*peer, now_ms);
            }
            due
        })
        .collect()
}

/// Next number of sync attempts to skip after a consecutive authorization denial.
fn next_denial_backoff(previous: u32) -> u32 {
    previous.saturating_mul(2).clamp(1, 64)
}

struct SyncManagerShared {
    owner: ActorOwnedServiceRoot<SyncServiceManager, SyncCommand, SyncManagerState>,
    configured_peers: Mutex<Vec<DeviceId>>,
    /// Actual created service retained before any awaited start/publication.
    startup_service: Mutex<Option<Arc<SyncService>>>,
    /// Remaining sync attempts to skip after an authorization denial.
    denial_skip_remaining: std::sync::atomic::AtomicU32,
    /// Current backoff window (attempts), doubled on each consecutive denial.
    denial_backoff: std::sync::atomic::AtomicU32,
    /// Wall-clock ms of the last sync attempt per peer, for coalescing triggers.
    recent_peer_syncs: Mutex<HashMap<DeviceId, u64>>,
    /// Wall-clock ms of the last fact exchange per sibling device.
    recent_sibling_exchanges: Mutex<HashMap<DeviceId, u64>>,
    /// Sibling devices with an exchange currently running.
    sibling_exchanges_in_flight: Mutex<std::collections::HashSet<DeviceId>>,
}

#[derive(Clone)]
struct SyncStateSnapshot {
    service: Option<Arc<SyncService>>,
    status: SyncManagerState,
    peers: Vec<DeviceId>,
}

enum SyncCommand {
    SnapshotState {
        reply: oneshot::Sender<SyncStateSnapshot>,
    },
    AddPeer {
        peer: DeviceId,
        reply: oneshot::Sender<()>,
    },
    RemovePeer {
        peer: DeviceId,
        reply: oneshot::Sender<()>,
    },
}

impl SyncState {
    fn new_running(service: Arc<SyncService>, initial_peers: Vec<DeviceId>) -> Self {
        Self {
            service,
            peers: initial_peers,
        }
    }
}

impl SyncManagerState {
    fn phase(self) -> ActorLifecyclePhase {
        match self {
            Self::Stopped => ActorLifecyclePhase::Stopped,
            Self::Starting => ActorLifecyclePhase::Starting,
            Self::Running => ActorLifecyclePhase::Running,
            Self::Stopping => ActorLifecyclePhase::Stopping,
            Self::Failed => ActorLifecyclePhase::Failed,
        }
    }
}

/// Manager for background journal synchronization
///
/// Integrates `aura_sync::SyncService` into the agent runtime lifecycle.
/// Handles startup, shutdown, and coordination with other agent services.
#[aura_macros::actor_owned(
    owner = "sync_service_manager",
    domain = "sync",
    gate = "sync_command_ingress",
    command = SyncCommand,
    capacity = 64,
    category = "actor_owned"
)]
#[derive(Clone)]
pub struct SyncServiceManager {
    /// Configuration
    config: SyncManagerConfig,
    /// Optional Merkle verifier for fact sync (requires indexed journal)
    merkle_verifier: Option<Arc<MerkleVerifier>>,
    /// Shared lifecycle boundary for service-local mutable state.
    shared: Arc<SyncManagerShared>,
    /// Reconfiguration/session-footprint state for sync choreography delegation.
    reconfiguration: ReconfigurationManager,
}

impl SyncServiceManager {
    pub const OWNERSHIP_CATEGORY: OwnershipCategory = OwnershipCategory::ActorOwned;

    async fn command_handle(
        &self,
    ) -> Result<ServiceActorHandle<SyncServiceManager, SyncCommand>, ServiceError> {
        self.shared
            .owner
            .command_handle(
                self.name(),
                "sync command actor unavailable; service is not fully started",
            )
            .await
    }

    fn spawn_command_actor(
        &self,
        tasks: &TaskGroup,
        mut state: SyncState,
    ) -> ServiceActorHandle<SyncServiceManager, SyncCommand> {
        let (commands, mut mailbox) =
            ServiceActorHandle::<SyncServiceManager, SyncCommand>::bounded(self.name(), 64);

        let _command_actor_handle = tasks.spawn_named("command_actor", async move {
            while let Some(command) = mailbox.recv().await {
                match command {
                    SyncCommand::SnapshotState { reply } => {
                        let _ = reply.send(SyncStateSnapshot {
                            service: Some(state.service.clone()),
                            status: SyncManagerState::Running,
                            peers: state.peers.clone(),
                        });
                    }
                    SyncCommand::AddPeer { peer, reply } => {
                        if !state.peers.contains(&peer) {
                            state.peers.push(peer);
                            tracing::debug!("Added peer {} to sync manager", peer);
                        }
                        let _ = reply.send(());
                    }
                    SyncCommand::RemovePeer { peer, reply } => {
                        state.peers.retain(|p| p != &peer);
                        tracing::debug!("Removed peer {} from sync manager", peer);
                        let _ = reply.send(());
                    }
                }
            }
        });

        commands
    }

    async fn state_snapshot(&self) -> Result<SyncStateSnapshot, ServiceError> {
        match self.command_handle().await {
            Ok(commands) => {
                commands
                    .request(|reply| SyncCommand::SnapshotState { reply })
                    .await
            }
            Err(source) => {
                let state = self.shared.owner.state().await;
                if state != SyncManagerState::Stopped {
                    return Err(source);
                }
                Ok(SyncStateSnapshot {
                    service: None,
                    status: state,
                    peers: self.shared.configured_peers.lock().await.clone(),
                })
            }
        }
    }

    fn shared_for_config(config: &SyncManagerConfig) -> Arc<SyncManagerShared> {
        Arc::new(SyncManagerShared {
            owner: ActorOwnedServiceRoot::new(SyncManagerState::Stopped),
            configured_peers: Mutex::new(config.initial_peers.clone()),
            startup_service: Mutex::new(None),
            denial_skip_remaining: std::sync::atomic::AtomicU32::new(0),
            denial_backoff: std::sync::atomic::AtomicU32::new(0),
            recent_peer_syncs: Mutex::new(HashMap::new()),
            recent_sibling_exchanges: Mutex::new(HashMap::new()),
            sibling_exchanges_in_flight: Mutex::new(std::collections::HashSet::new()),
        })
    }

    /// Create a new sync service manager
    pub fn new(config: SyncManagerConfig) -> Self {
        Self {
            config: config.clone(),
            merkle_verifier: None,
            shared: Self::shared_for_config(&config),
            reconfiguration: ReconfigurationManager::new(),
        }
    }

    /// Create a new sync service manager with indexed journal for Merkle verification
    ///
    /// This enables fact sync with cryptographic verification of facts using
    /// Merkle trees and Bloom filters from the indexed journal.
    pub fn with_indexed_journal(
        config: SyncManagerConfig,
        indexed_journal: Arc<dyn IndexedJournalEffects + Send + Sync>,
        time: Arc<dyn PhysicalTimeEffects>,
    ) -> Self {
        Self {
            config: config.clone(),
            merkle_verifier: Some(Arc::new(MerkleVerifier::new(indexed_journal, time))),
            shared: Self::shared_for_config(&config),
            reconfiguration: ReconfigurationManager::new(),
        }
    }

    /// Create with default configuration
    pub fn with_defaults() -> Self {
        Self::new(SyncManagerConfig::default())
    }

    /// Get the current state
    pub async fn state(&self) -> SyncManagerState {
        self.state_snapshot()
            .await
            .map(|snapshot| snapshot.status)
            .unwrap_or(self.shared.owner.state().await)
    }

    /// Check if the service is running
    pub async fn is_running(&self) -> bool {
        self.state().await == SyncManagerState::Running
    }

    async fn start_managed(&self, context: &RuntimeServiceContext) -> Result<(), ServiceError> {
        let max_concurrent_syncs =
            u32::try_from(self.config.max_concurrent_syncs).map_err(|source| {
                ServiceError::new(
                    self.name(),
                    ServiceErrorKind::InvalidConfiguration,
                    "sync concurrent capacity cannot be represented",
                )
                .with_cause(source)
            })?;
        if max_concurrent_syncs == 0 {
            return Err(ServiceError::new(
                self.name(),
                ServiceErrorKind::InvalidConfiguration,
                "sync concurrent capacity must be positive",
            ));
        }

        let _lifecycle_guard = self.shared.owner.lifecycle().lock().await;
        let current_state = self.shared.owner.state().await;
        if current_state == SyncManagerState::Running {
            return Ok(());
        }
        validate_actor_transition(
            self.name(),
            current_state.phase(),
            ActorLifecyclePhase::Starting,
        )?;

        self.shared
            .owner
            .set_state(SyncManagerState::Starting)
            .await;

        // Build aura-sync service config from our config
        let sync_config = SyncServiceConfig {
            auto_sync_enabled: self.config.auto_sync_enabled,
            auto_sync_interval: self.config.auto_sync_interval,
            max_concurrent_syncs,
            journal_sync: self.config.journal_sync.clone(),
            ..Default::default()
        };

        // Create the underlying sync service
        let now_instant = SyncService::monotonic_now();
        let service = match SyncService::new(sync_config, context.time_effects(), now_instant).await
        {
            Ok(service) => service,
            Err(error) => {
                self.shared.owner.set_state(SyncManagerState::Failed).await;
                return Err(
                    ServiceError::startup_failed("sync_service", error.to_string())
                        .with_cause(error),
                );
            }
        };

        let service = Arc::new(service);
        *self.shared.startup_service.lock().await = Some(service.clone());
        // Start the service
        if let Err(error) = service.start(now_instant).await {
            self.shared.owner.set_state(SyncManagerState::Failed).await;
            return Err(
                ServiceError::startup_failed("sync_service", error.to_string()).with_cause(error),
            );
        }

        let initial_peers = self.shared.configured_peers.lock().await.clone();
        let maintenance_group = context.tasks().group(self.name());
        self.shared
            .owner
            .install_tasks(maintenance_group.clone())
            .await;
        let command_handle = self.spawn_command_actor(
            &maintenance_group,
            SyncState::new_running(service, initial_peers),
        );
        self.shared.owner.install_commands(command_handle).await;
        self.spawn_maintenance_task(maintenance_group.clone(), context.time_effects());
        self.shared.owner.set_state(SyncManagerState::Running).await;
        // The actual group was installed before any task spawn.

        tracing::info!(
            event = "runtime.service.sync.started",
            service = self.name(),
            "Sync service manager started"
        );
        Ok(())
    }

    pub(crate) fn shares_owner(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.shared, &other.shared)
    }

    #[aura_macros::capability_boundary(category = "capability_gated",
        capability = "SyncCommandStopCapability", capability_type = super::sync_command_registry::SyncCommandStopCapability,
        family = "runtime_helper")]
    pub(crate) async fn stop_registered_command(
        &self,
        original: &super::sync_command_registry::SyncCommandStopCapability<'_>,
    ) -> Result<(), ServiceError> {
        original.require_manager(self)?;
        self.stop_managed_with_command(Some(original)).await
    }

    async fn stop_managed(&self) -> Result<(), ServiceError> {
        self.stop_managed_with_command(None).await
    }

    async fn stop_managed_with_command(
        &self,
        original: Option<&super::sync_command_registry::SyncCommandStopCapability<'_>>,
    ) -> Result<(), ServiceError> {
        let _lifecycle = self.shared.owner.lifecycle().lock().await;
        let state = self.shared.owner.state().await;
        if state == SyncManagerState::Stopped {
            return Ok(());
        }
        if matches!(state, SyncManagerState::Starting | SyncManagerState::Failed) {
            // Only the actual registered command owner may clean an interrupted
            // start. This is resource retirement, not a new running admission.
            let original = original.ok_or_else(|| {
                ServiceError::shutdown_failed(
                    self.name(),
                    "partial startup requires retained registered command owner",
                )
            })?;
            original.require_manager(self)?;
        } else {
            validate_actor_transition(self.name(), state.phase(), ActorLifecyclePhase::Stopping)?;
        }
        self.shared
            .owner
            .set_state(SyncManagerState::Stopping)
            .await;
        let service = self.shared.startup_service.lock().await.clone();
        let mut failures = Vec::new();
        if let Some(service) = service {
            if let Err(source) = service.stop(SyncService::monotonic_now()).await {
                failures.push(
                    ServiceError::shutdown_failed(self.name(), "underlying sync stop failed")
                        .with_cause(source),
                );
            }
        }
        // Actual owner retains the service separately; no failed actor snapshot
        // can manufacture an absent service and skip required retirement.
        self.shared.owner.take_commands().await;
        if let Some(group) = self.shared.owner.retained_tasks().await {
            let completion = match original {
                Some(original) => original.shutdown_tasks(&group).await,
                None => group
                    .shutdown_with_timeout(Duration::from_secs(2))
                    .await
                    .map_err(|source| {
                        ServiceError::shutdown_failed(
                            self.name(),
                            "standalone sync workers did not acknowledge completion",
                        )
                        .with_cause(source)
                    }),
            };
            match completion {
                Ok(()) => {
                    self.shared.owner.take_tasks().await;
                }
                Err(source) => failures.push(source),
            }
        }
        if failures.is_empty() {
            self.shared.owner.set_state(SyncManagerState::Stopped).await;
            Ok(())
        } else {
            self.shared.owner.set_state(SyncManagerState::Failed).await;
            let primary = failures.remove(0);
            if failures.is_empty() {
                Err(primary)
            } else {
                Err(ServiceError::shutdown_failed(
                    self.name(),
                    "sync stop failed at multiple required boundaries",
                )
                .with_cause(SyncStopFailures {
                    primary,
                    cleanup: failures,
                }))
            }
        }
    }

    /// Start background maintenance task for pruning long-lived state.
    fn spawn_maintenance_task(
        &self,
        tasks: TaskGroup,
        time_effects: Arc<dyn PhysicalTimeEffects + Send + Sync>,
    ) {
        if !self.config.maintenance_enabled {
            tracing::debug!("Sync maintenance task disabled by configuration");
            return;
        }

        let interval = self.config.maintenance_interval;
        let peer_state_ttl = self.config.peer_state_ttl;
        let max_peer_states = self.config.max_peer_states;
        let manager = self.clone();

        let _maintenance_task_handle = tasks.spawn_try_interval_until_named(
            "sync.maintenance",
            time_effects.clone(),
            interval,
            move || {
                let manager = manager.clone();
                let time_effects = time_effects.clone();
                async move {
                    let snapshot = manager.state_snapshot().await.map_err(|source| {
                        aura_core::AuraError::Internal {
                            message: "required sync maintenance actor read failed".into(),
                            source: Some(Arc::new(source)),
                        }
                    })?;
                    if matches!(
                        snapshot.status,
                        SyncManagerState::Stopped | SyncManagerState::Stopping
                    ) {
                        return Ok(false);
                    }
                    if snapshot.status == SyncManagerState::Failed {
                        return Err(aura_core::AuraError::Internal {
                            message: "sync maintenance owner entered failed state".into(),
                            source: Some(Arc::new(SyncManagerReadinessError::ManagerFailed)),
                        });
                    }
                    let now = time_effects.physical_time().await.map_err(|source| {
                        aura_core::AuraError::Internal {
                            message: "required sync maintenance physical observation failed".into(),
                            source: Some(Arc::new(source)),
                        }
                    })?;
                    let ttl_ms = u64::try_from(peer_state_ttl.as_millis()).map_err(|source| {
                        aura_core::AuraError::Invalid {
                            message: "sync maintenance TTL cannot be represented".into(),
                            source: Some(Arc::new(source)),
                        }
                    })?;
                    let service =
                        snapshot
                            .service
                            .ok_or_else(|| aura_core::AuraError::Internal {
                                message: "running sync maintenance owner lost backing service"
                                    .into(),
                                source: Some(Arc::new(SyncManagerReadinessError::MissingService)),
                            })?;
                    service
                        .maintenance_cleanup(now.ts_ms, ttl_ms, max_peer_states)
                        .await?;
                    Ok(true)
                }
            },
        );
    }

    /// Install the authority Biscuit authorization on the running sync service
    /// if it is not yet configured.
    ///
    /// Anti-entropy denies every peer without it, so callers invoke this before
    /// each sync; it is a no-op once installed or before bootstrap.
    pub async fn ensure_biscuit_authorization(
        &self,
        effects: &crate::runtime::AuraEffectSystem,
    ) -> Result<(), SyncManagerError> {
        let service = self
            .state_snapshot()
            .await
            .map_err(SyncManagerError::from)?
            .service
            .clone()
            .ok_or(SyncManagerError::NotStarted)?;
        if service.has_biscuit_authorization() {
            return Ok(());
        }
        match effects.sync_biscuit_authorization() {
            Ok(Some((token_manager, guard_evaluator))) => {
                service.install_biscuit_authorization(token_manager, guard_evaluator);
                tracing::info!(
                    event = "runtime.service.sync.authorization_installed",
                    "Installed Biscuit authorization for journal sync"
                );
            }
            Ok(None) => {}
            Err(error) => return Err(SyncManagerError::NativeSync(error)),
        }
        Ok(())
    }

    pub(crate) async fn required_tracked_peers(&self) -> Result<Vec<DeviceId>, SyncManagerError> {
        let snapshot = self
            .state_snapshot()
            .await
            .map_err(SyncManagerError::from)?;
        if snapshot.status != SyncManagerState::Running {
            return Err(SyncManagerError::NotStarted);
        }
        Ok(snapshot.peers)
    }

    /// Required requested-peer execution keeps the actual caller's original
    /// resource window. Recent-sync observation cannot turn requested work into
    /// an unperformed successful result.
    pub(crate) async fn sync_requested_peers_in_original_window<E>(
        &self,
        effects: &E,
        peers: Vec<DeviceId>,
        original: &aura_core::time::timeout::TimeoutBudget,
    ) -> Result<(), SyncManagerError>
    where
        E: aura_core::effects::JournalEffects
            + aura_core::effects::NetworkEffects
            + aura_core::effects::PhysicalTimeEffects
            + aura_protocol::effects::TreeEffects
            + aura_guards::GuardContextProvider
            + Send
            + Sync,
    {
        let service = self
            .state_snapshot()
            .await
            .map_err(SyncManagerError::from)?
            .service
            .ok_or(SyncManagerError::NotStarted)?;
        service
            .sync_with_peers_in_original_window(
                effects,
                peers,
                SyncService::monotonic_now(),
                original,
            )
            .await
            .map_err(SyncManagerError::NativeSync)
    }

    /// Perform a manual sync with specific peers
    ///
    /// # Arguments
    /// - `effects`: Effect system providing journal, network, and time capabilities
    /// - `peers`: List of peers to sync with
    pub async fn sync_with_peers<E>(
        &self,
        effects: &E,
        peers: Vec<DeviceId>,
    ) -> Result<(), SyncManagerError>
    where
        E: aura_core::effects::JournalEffects
            + aura_core::effects::NetworkEffects
            + aura_core::effects::PhysicalTimeEffects
            + aura_protocol::effects::TreeEffects
            + aura_guards::GuardContextProvider
            + Send
            + Sync,
    {
        let service = self
            .state_snapshot()
            .await
            .map_err(SyncManagerError::from)?
            .service
            .clone()
            .ok_or(SyncManagerError::NotStarted)?;

        use std::sync::atomic::Ordering;
        // Back off after authorization denials instead of retrying every tick.
        if self
            .shared
            .denial_skip_remaining
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |n| n.checked_sub(1))
            .is_ok()
        {
            return Err(SyncManagerError::AuthorizationBackoff);
        }

        let now_ms = effects
            .physical_time()
            .await
            .map(|time| time.ts_ms)
            .map_err(SyncManagerError::RequiredClock)?;
        let peers = take_due_peers(
            &mut *self.shared.recent_peer_syncs.lock().await,
            peers,
            now_ms,
        );
        if peers.is_empty() {
            return Ok(());
        }

        let now_instant = SyncService::monotonic_now();
        let result = service
            .sync_with_peers(effects, peers, now_instant)
            .await
            .map_err(SyncManagerError::NativeSync);
        match &result {
            Err(SyncManagerError::NativeSync(source)) if sync_authorization_denial(source) => {
                let next = next_denial_backoff(self.shared.denial_backoff.load(Ordering::Acquire));
                self.shared.denial_backoff.store(next, Ordering::Release);
                self.shared
                    .denial_skip_remaining
                    .store(next, Ordering::Release);
            }
            Ok(()) => self.shared.denial_backoff.store(0, Ordering::Release),
            Err(_) => {}
        }
        result
    }

    /// Sibling devices due for a fact exchange, coalesced like peer syncs.
    pub async fn take_due_siblings(&self, siblings: Vec<DeviceId>, now_ms: u64) -> Vec<DeviceId> {
        take_due_peers(
            &mut *self.shared.recent_sibling_exchanges.lock().await,
            siblings,
            now_ms,
        )
    }

    /// Claim a sibling for one exchange; false if one is already running.
    pub async fn begin_sibling_exchange(&self, sibling: DeviceId) -> bool {
        self.shared
            .sibling_exchanges_in_flight
            .lock()
            .await
            .insert(sibling)
    }

    /// Release a sibling claimed by `begin_sibling_exchange`.
    pub async fn end_sibling_exchange(&self, sibling: DeviceId) {
        self.shared
            .sibling_exchanges_in_flight
            .lock()
            .await
            .remove(&sibling);
    }

    /// Add a peer to the known peers list
    pub async fn add_peer(&self, peer: DeviceId) {
        if let Ok(commands) = self.command_handle().await {
            let _ = commands
                .request(|reply| SyncCommand::AddPeer { peer, reply })
                .await;
        } else {
            let mut peers = self.shared.configured_peers.lock().await;
            if !peers.contains(&peer) {
                peers.push(peer);
                tracing::debug!("Added peer {} to sync manager", peer);
            }
        }
    }

    /// Remove a peer from the known peers list
    pub async fn remove_peer(&self, peer: &DeviceId) {
        if let Ok(commands) = self.command_handle().await {
            let _ = commands
                .request(|reply| SyncCommand::RemovePeer { peer: *peer, reply })
                .await;
        } else {
            let mut peers = self.shared.configured_peers.lock().await;
            peers.retain(|p| p != peer);
            tracing::debug!("Removed peer {} from sync manager", peer);
        }
    }

    /// Get the list of known peers
    pub async fn peers(&self) -> Vec<DeviceId> {
        self.state_snapshot()
            .await
            .map(|snapshot| snapshot.peers)
            .unwrap_or_default()
    }

    /// Get service health information
    pub async fn sync_service_health(&self) -> Option<aura_sync::services::SyncServiceHealth> {
        self.state_snapshot()
            .await
            .ok()
            .and_then(|snapshot| snapshot.service.map(|s| s.get_health()))
    }

    /// Get service metrics
    pub async fn metrics(&self) -> Option<aura_sync::services::ServiceMetrics> {
        self.state_snapshot()
            .await
            .ok()
            .and_then(|snapshot| snapshot.service.map(|s| s.get_metrics()))
    }

    /// Required startup readiness reads the actual actor/service; observed
    /// optional health and metrics do not repair missing running custody.
    pub(crate) async fn required_running_health(&self) -> Result<ServiceHealth, ServiceError> {
        let snapshot = self.state_snapshot().await?;
        let service = snapshot.service.ok_or_else(|| {
            ServiceError::unavailable(self.name(), "required running sync service is absent")
                .with_cause(SyncManagerReadinessError::MissingService)
        })?;
        let health = service.get_health();
        use aura_sync::services::HealthStatus;
        match health.status {
            HealthStatus::Healthy => Ok(ServiceHealth::Healthy),
            HealthStatus::Degraded => Ok(ServiceHealth::Degraded {
                reason: "actual underlying sync health degraded".into(),
            }),
            status
            @ (HealthStatus::Starting | HealthStatus::Unhealthy | HealthStatus::Stopping) => Err(
                ServiceError::unavailable(self.name(), "required sync service is not operational")
                    .with_cause(SyncManagerReadinessError::NonOperational { status }),
            ),
        }
    }

    /// Get the configuration
    pub fn config(&self) -> &SyncManagerConfig {
        &self.config
    }

    // =========================================================================
    // Merkle Verification Methods
    // =========================================================================

    /// Check if Merkle verification is available
    ///
    /// Returns `true` if the manager was created with an indexed journal,
    /// enabling cryptographic fact verification.
    pub fn has_merkle_verification(&self) -> bool {
        self.merkle_verifier.is_some()
    }

    /// Get the local Merkle root for exchange with peers
    ///
    /// Returns `None` if Merkle verification is not enabled (no indexed journal).
    /// The root represents the current state of the local fact journal and can
    /// be compared with remote roots to determine if synchronization is needed.
    pub async fn local_merkle_root(&self) -> Option<[u8; 32]> {
        if let Some(ref verifier) = self.merkle_verifier {
            verifier.local_merkle_root().await.ok()
        } else {
            None
        }
    }

    /// Test-only legacy raw fact verification hook retained to assert that
    /// proofless sync admission stays fail-closed.
    #[cfg(test)]
    pub async fn verify_facts(
        &self,
        facts: Vec<IndexedFact>,
        claimed_root: [u8; 32],
    ) -> Option<VerificationResult> {
        let _ = (&self.merkle_verifier, facts, claimed_root);
        None
    }

    /// Get the internal Merkle verifier reference
    ///
    /// Returns `None` if Merkle verification is not enabled.
    /// Use this for direct access to verification operations like
    /// `compare_roots()` or `local_bloom_filter()`.
    pub fn merkle_verifier(&self) -> Option<&Arc<MerkleVerifier>> {
        self.merkle_verifier.as_ref()
    }
}

impl Default for SyncServiceManager {
    fn default() -> Self {
        Self::with_defaults()
    }
}

// =============================================================================
// RuntimeService Implementation
// =============================================================================

#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
impl RuntimeService for SyncServiceManager {
    fn name(&self) -> &'static str {
        "sync_service"
    }

    fn dependencies(&self) -> &[&'static str] {
        &["indexed_journal", "transport"]
    }

    async fn start(&self, context: &RuntimeServiceContext) -> Result<(), ServiceError> {
        self.start_managed(context).await
    }

    async fn stop(&self) -> Result<(), ServiceError> {
        self.stop_managed().await
    }

    async fn health(&self) -> ServiceHealth {
        self.health_async().await
    }
}

/// Async health check implementation
impl SyncServiceManager {
    /// Get the service health status asynchronously
    ///
    /// This provides full health information including underlying service health.
    /// For the synchronous `RuntimeService::health()`, a simplified status is returned.
    pub async fn health_async(&self) -> ServiceHealth {
        let state = self.state().await;
        match state {
            SyncManagerState::Stopped => ServiceHealth::Stopped,
            SyncManagerState::Starting => ServiceHealth::Starting,
            SyncManagerState::Stopping => ServiceHealth::Stopping,
            SyncManagerState::Failed => ServiceHealth::Unhealthy {
                reason: "service entered failed lifecycle state".to_string(),
            },
            SyncManagerState::Running => {
                // Check underlying service health
                if let Some(svc_health) = self.sync_service_health().await {
                    use aura_sync::services::HealthStatus;
                    match svc_health.status {
                        HealthStatus::Healthy => ServiceHealth::Healthy,
                        HealthStatus::Degraded | HealthStatus::Starting => {
                            ServiceHealth::Degraded {
                                reason: format!(
                                    "status={:?}, active_sessions={}",
                                    svc_health.status, svc_health.active_sessions
                                ),
                            }
                        }
                        HealthStatus::Unhealthy | HealthStatus::Stopping => {
                            ServiceHealth::Unhealthy {
                                reason: format!("status={:?}", svc_health.status),
                            }
                        }
                    }
                } else {
                    ServiceHealth::Unhealthy {
                        reason: "underlying service not available".to_string(),
                    }
                }
            }
        }
    }
}

// =============================================================================
// Choreography Wiring (execute_as)
// =============================================================================

impl SyncServiceManager {
    fn epoch_role(authority_id: AuthorityId, role_index: u16) -> ChoreographicRole {
        ChoreographicRole::for_authority(
            authority_id,
            RoleIndex::new(role_index.into()).expect("role index"),
        )
    }

    /// Execute epoch rotation protocol as coordinator.
    pub async fn execute_epoch_rotation_coordinator(
        &self,
        effects: Arc<AuraEffectSystem>,
        coordinator_id: AuthorityId,
        participant1_id: AuthorityId,
        participant2_id: AuthorityId,
        proposal: EpochRotationProposal,
        commit: EpochCommit,
    ) -> Result<(), SyncManagerError> {
        let session_id = epoch_rotation_session_id(&proposal.rotation_id);
        self.record_native_epoch_session(coordinator_id, session_id)
            .await;
        let roles = vec![
            Self::epoch_role(coordinator_id, 0),
            Self::epoch_role(participant1_id, 0),
            Self::epoch_role(participant2_id, 0),
        ];
        let peer_roles = BTreeMap::from([
            (
                "Participant1".to_string(),
                Self::epoch_role(participant1_id, 0),
            ),
            (
                "Participant2".to_string(),
                Self::epoch_role(participant2_id, 0),
            ),
        ]);
        let manifest =
            aura_sync::protocols::epochs::telltale_session_types_epoch_rotation::vm_artifacts::composition_manifest();
        let global_type =
            aura_sync::protocols::epochs::telltale_session_types_epoch_rotation::vm_artifacts::global_type();
        let local_types =
            aura_sync::protocols::epochs::telltale_session_types_epoch_rotation::vm_artifacts::local_types();

        let result = async {
            let mut session = open_owned_manifest_vm_session_admitted(
                effects.clone(),
                session_id,
                roles,
                &manifest,
                "Coordinator",
                &global_type,
                &local_types,
                crate::runtime::AuraVmSchedulerSignals::default(),
            )
            .await
            .map_err(|error| SyncManagerError::VmSessionOpen(error.to_string()))?;
            session.queue_send_bytes(
                to_vec(&proposal)
                    .map_err(|error| SyncManagerError::ProposalEncode(error.to_string()))?,
            );
            let mut confirmations = Vec::new();
            let mut commit_queued = false;

            let loop_result = loop {
                let round = session
                    .advance_round("Coordinator", &peer_roles)
                    .await
                    .map_err(|error| SyncManagerError::VmAdvance(error.to_string()))?;

                if let Some(blocked) = round.blocked_receive {
                    let confirmation: EpochConfirmation =
                        aura_core::util::serialization::from_slice(&blocked.payload).map_err(
                            |error| SyncManagerError::ConfirmationDecode(error.to_string()),
                        )?;
                    confirmations.push(confirmation);
                    if !commit_queued && confirmations.len() == 2 {
                        let payload = to_vec(&commit)
                            .map_err(|error| SyncManagerError::CommitEncode(error.to_string()))?;
                        session.queue_send_bytes(payload.clone());
                        session.queue_send_bytes(payload);
                        commit_queued = true;
                    }
                    session
                        .inject_blocked_receive(&blocked)
                        .map_err(|error| SyncManagerError::VmRoundHandling(error.to_string()))?;
                    continue;
                }

                match round.host_wait_status {
                    AuraVmHostWaitStatus::Idle => {}
                    AuraVmHostWaitStatus::TimedOut => {
                        break Err(SyncManagerError::VmTimedOut {
                            role: "coordinator",
                        });
                    }
                    AuraVmHostWaitStatus::Cancelled => {
                        break Err(SyncManagerError::VmCancelled {
                            role: "coordinator",
                        });
                    }
                    AuraVmHostWaitStatus::Deferred | AuraVmHostWaitStatus::Delivered => {}
                }

                match round.step {
                    StepResult::AllDone => break Ok(()),
                    StepResult::Continue => {}
                    StepResult::Stuck => {
                        break Err(SyncManagerError::VmStuck {
                            role: "coordinator",
                        });
                    }
                }
            };
            let _ = session.close().await;
            loop_result
        }
        .await;
        result
    }

    /// Execute epoch rotation protocol as participant.
    pub async fn execute_epoch_rotation_participant(
        &self,
        effects: Arc<AuraEffectSystem>,
        role: EpochRotationProtocolRole,
        coordinator_id: AuthorityId,
        participant1_id: AuthorityId,
        participant2_id: AuthorityId,
        confirmation: EpochConfirmation,
    ) -> Result<(), SyncManagerError> {
        let (participant_id, active_role_name) = match role {
            EpochRotationProtocolRole::Participant1 => (participant1_id, "Participant1"),
            EpochRotationProtocolRole::Participant2 => (participant2_id, "Participant2"),
            EpochRotationProtocolRole::Coordinator => {
                return Err(SyncManagerError::ParticipantRoleRequired)
            }
        };
        let session_id = epoch_rotation_session_id(&confirmation.rotation_id);
        self.record_native_epoch_session(participant_id, session_id)
            .await;
        let roles = vec![
            Self::epoch_role(coordinator_id, 0),
            Self::epoch_role(participant_id, 0),
        ];
        let peer_roles = BTreeMap::from([(
            "Coordinator".to_string(),
            Self::epoch_role(coordinator_id, 0),
        )]);
        let manifest =
            aura_sync::protocols::epochs::telltale_session_types_epoch_rotation::vm_artifacts::composition_manifest();
        let global_type =
            aura_sync::protocols::epochs::telltale_session_types_epoch_rotation::vm_artifacts::global_type();
        let local_types =
            aura_sync::protocols::epochs::telltale_session_types_epoch_rotation::vm_artifacts::local_types();

        let result = async {
            let mut session = open_owned_manifest_vm_session_admitted(
                effects.clone(),
                session_id,
                roles,
                &manifest,
                active_role_name,
                &global_type,
                &local_types,
                crate::runtime::AuraVmSchedulerSignals::default(),
            )
            .await
            .map_err(|error| SyncManagerError::VmSessionOpen(error.to_string()))?;
            session.queue_send_bytes(
                to_vec(&confirmation)
                    .map_err(|error| SyncManagerError::ConfirmationEncode(error.to_string()))?,
            );

            let loop_result = loop {
                let round = session
                    .advance_round(active_role_name, &peer_roles)
                    .await
                    .map_err(|error| SyncManagerError::VmAdvance(error.to_string()))?;

                match crate::runtime::handle_owned_vm_round(
                    &mut session,
                    round,
                    "epoch rotation participant VM",
                )
                .map_err(|error| SyncManagerError::VmRoundHandling(error.to_string()))?
                {
                    AuraVmRoundDisposition::Continue => {}
                    AuraVmRoundDisposition::Complete => break Ok(()),
                }
            };
            let _ = session.close().await;
            loop_result
        }
        .await;
        result
    }

    /// Delegate ownership of an epoch-rotation session to another authority.
    ///
    /// This updates runtime session footprints and records a relational protocol fact.
    pub async fn delegate_epoch_rotation_session(
        &self,
        effects: Arc<AuraEffectSystem>,
        rotation_id: &str,
        from_authority: AuthorityId,
        to_authority: AuthorityId,
        bundle_id: Option<String>,
    ) -> Result<DelegationReceipt, SyncManagerError> {
        let session_id =
            RuntimeChoreographySessionId::from_uuid(epoch_rotation_session_id(rotation_id))
                .into_aura_session_id();
        self.reconfiguration
            .record_native_session(from_authority, session_id)
            .await;
        self.reconfiguration
            .delegate_session(
                &effects,
                SessionDelegationTransfer::new(
                    session_id,
                    from_authority,
                    to_authority,
                    bundle_id.ok_or(SyncManagerError::DelegationRequiresBundleEvidence)?,
                )
                .with_context(default_context_id_for_authority(from_authority)),
            )
            .await
            .map(|outcome| outcome.receipt)
            .map_err(SyncManagerError::Delegation)
    }

    async fn record_native_epoch_session(&self, authority_id: AuthorityId, session_uuid: Uuid) {
        let session_id =
            RuntimeChoreographySessionId::from_uuid(session_uuid).into_aura_session_id();
        self.reconfiguration
            .record_native_session(authority_id, session_id)
            .await;
    }
}

fn epoch_rotation_session_id(rotation_id: &str) -> Uuid {
    let digest = hash(rotation_id.as_bytes());
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    Uuid::from_bytes(bytes)
}

/// Only an actual retained authorization outcome may activate denial backoff.
fn sync_authorization_denial(error: &aura_core::AuraError) -> bool {
    let mut source: Option<&(dyn std::error::Error + 'static)> = Some(error);
    while let Some(cause) = source {
        if matches!(
            cause.downcast_ref::<aura_core::AuraError>(),
            Some(aura_core::AuraError::PermissionDenied { .. })
        ) {
            return true;
        }
        source = cause.source();
    }
    false
}

#[derive(Debug, thiserror::Error)]
#[error("{primary}; {} additional sync cleanup failures", cleanup.len())]
struct SyncStopFailures {
    #[source]
    primary: ServiceError,
    cleanup: Vec<ServiceError>,
}

#[derive(Debug, thiserror::Error)]
enum SyncManagerReadinessError {
    #[error("actual sync manager entered failed lifecycle state")]
    ManagerFailed,
    #[error("actual running sync actor has no backing service")]
    MissingService,
    #[error("actual sync service status is {status:?}")]
    NonOperational {
        status: aura_sync::services::HealthStatus,
    },
}

#[cfg(test)]
#[allow(clippy::disallowed_types)]
mod tests {
    use super::*;

    // Regression (work/8.md task 3): workflows trigger sync constantly; a
    // trigger inside the resync window must not start another exchange.
    #[test]
    fn sync_triggers_within_resync_window_are_coalesced() {
        let peer = DeviceId::new_from_entropy([0x31; 32]);
        let other = DeviceId::new_from_entropy([0x32; 32]);
        let mut recent = HashMap::new();

        assert_eq!(take_due_peers(&mut recent, vec![peer], 1_000), vec![peer]);
        assert_eq!(
            take_due_peers(&mut recent, vec![peer, other], 2_000),
            vec![other]
        );
        assert_eq!(
            take_due_peers(&mut recent, vec![peer], 1_000 + MIN_PEER_RESYNC_INTERVAL_MS),
            vec![peer]
        );
    }

    #[test]
    fn denial_backoff_grows_exponentially_and_caps() {
        let mut window = 0;
        let mut seen = Vec::new();
        for _ in 0..9 {
            window = next_denial_backoff(window);
            seen.push(window);
        }
        assert_eq!(seen, vec![1, 2, 4, 8, 16, 32, 64, 64, 64]);
    }
    use async_trait::async_trait;
    use aura_core::domain::journal::FactValue;
    use aura_core::effects::indexed::{FactId, FactStreamReceiver, IndexStats};
    use aura_core::effects::{BloomConfig, BloomFilter};
    use aura_core::AuthorityId;
    use aura_effects::time::PhysicalTimeHandler;
    use std::sync::Mutex;

    async fn test_service_context() -> RuntimeServiceContext {
        let time: Arc<dyn aura_core::effects::PhysicalTimeEffects + Send + Sync> =
            Arc::new(PhysicalTimeHandler::new());
        let original = aura_core::TimeoutBudget::from_start_and_timeout(
            &time
                .physical_time()
                .await
                .expect("actual service fixture startup observation"),
            std::time::Duration::from_secs(30),
        )
        .expect("original actual service fixture window");
        RuntimeServiceContext::new(
            Arc::new(crate::runtime::TaskSupervisor::new()),
            time,
            original,
        )
    }

    /// Mock indexed journal for testing
    struct MockIndexedJournal {
        root: Mutex<[u8; 32]>,
        facts: Mutex<Vec<IndexedFact>>,
    }

    struct EmptyFactStreamReceiver;

    impl FactStreamReceiver for EmptyFactStreamReceiver {
        fn recv(
            &mut self,
        ) -> std::pin::Pin<
            Box<
                dyn std::future::Future<Output = Result<Vec<IndexedFact>, aura_core::AuraError>>
                    + Send
                    + '_,
            >,
        > {
            Box::pin(async { Ok(Vec::new()) })
        }

        fn try_recv(&mut self) -> Result<Option<Vec<IndexedFact>>, aura_core::AuraError> {
            Ok(None)
        }
    }

    impl MockIndexedJournal {
        fn new(root: [u8; 32]) -> Self {
            Self {
                root: Mutex::new(root),
                facts: Mutex::new(Vec::new()),
            }
        }
    }

    #[async_trait]
    impl IndexedJournalEffects for MockIndexedJournal {
        fn watch_facts(&self) -> Box<dyn FactStreamReceiver> {
            Box::new(EmptyFactStreamReceiver)
        }

        async fn facts_by_predicate(
            &self,
            _predicate: &str,
        ) -> Result<Vec<IndexedFact>, aura_core::AuraError> {
            Ok(Vec::new())
        }

        async fn facts_by_authority(
            &self,
            _authority: &AuthorityId,
        ) -> Result<Vec<IndexedFact>, aura_core::AuraError> {
            Ok(Vec::new())
        }

        async fn facts_in_range(
            &self,
            _start: aura_core::time::TimeStamp,
            _end: aura_core::time::TimeStamp,
        ) -> Result<Vec<IndexedFact>, aura_core::AuraError> {
            Ok(Vec::new())
        }

        async fn all_facts(&self) -> Result<Vec<IndexedFact>, aura_core::AuraError> {
            Ok(self.facts.lock().unwrap().clone())
        }

        fn might_contain(&self, _predicate: &str, _value: &FactValue) -> bool {
            false
        }

        async fn merkle_root(&self) -> Result<[u8; 32], aura_core::AuraError> {
            Ok(*self.root.lock().unwrap())
        }

        async fn verify_fact_inclusion(
            &self,
            fact: &IndexedFact,
        ) -> Result<bool, aura_core::AuraError> {
            let facts = self.facts.lock().unwrap();
            Ok(facts.iter().any(|f| f.id == fact.id))
        }

        async fn get_bloom_filter(&self) -> Result<BloomFilter, aura_core::AuraError> {
            BloomFilter::new(BloomConfig::for_sync(100))
        }

        async fn index_stats(&self) -> Result<IndexStats, aura_core::AuraError> {
            let facts = self.facts.lock().unwrap();
            Ok(IndexStats {
                fact_count: facts.len() as u64,
                predicate_count: 1,
                authority_count: 1,
                bloom_fp_rate: 0.01,
                merkle_depth: 10,
            })
        }
    }

    #[tokio::test]
    async fn test_sync_manager_creation() {
        let config = SyncManagerConfig::for_testing();
        let manager = SyncServiceManager::new(config);

        assert_eq!(manager.state().await, SyncManagerState::Stopped);
        assert!(!manager.is_running().await);
    }

    #[tokio::test]
    async fn test_sync_manager_lifecycle() {
        let config = SyncManagerConfig::for_testing();
        let manager = SyncServiceManager::new(config);
        let context = test_service_context().await;

        // Start
        RuntimeService::start(&manager, &context).await.unwrap();
        assert!(manager.is_running().await);

        // Stop
        RuntimeService::stop(&manager).await.unwrap();
        assert!(!manager.is_running().await);
    }

    #[tokio::test]
    async fn test_sync_manager_concurrent_lifecycle_transitions_are_idempotent() {
        let manager = SyncServiceManager::new(SyncManagerConfig::for_testing());
        let context = test_service_context().await;

        let start_a = RuntimeService::start(&manager, &context);
        let start_b = RuntimeService::start(&manager, &context);
        let (start_a, start_b) = tokio::join!(start_a, start_b);
        start_a.expect("first concurrent start should succeed");
        start_b.expect("second concurrent start should be idempotent");
        assert!(manager.is_running().await);

        let stop_a = RuntimeService::stop(&manager);
        let stop_b = RuntimeService::stop(&manager);
        let (stop_a, stop_b) = tokio::join!(stop_a, stop_b);
        stop_a.expect("first concurrent stop should succeed");
        stop_b.expect("second concurrent stop should be idempotent");
        assert!(!manager.is_running().await);
    }

    #[tokio::test]
    async fn test_sync_manager_stop_drains_owned_tasks() {
        let manager = SyncServiceManager::new(SyncManagerConfig::for_testing());
        let context = test_service_context().await;

        RuntimeService::start(&manager, &context).await.unwrap();
        let task_group = manager
            .shared
            .owner
            .task_group()
            .await
            .expect("running sync service should own a maintenance task group");

        RuntimeService::stop(&manager).await.unwrap();
        task_group
            .wait_for_idle(Duration::from_secs(1))
            .await
            .expect("sync task group should drain on stop");
        assert!(
            task_group.active_tasks().is_empty(),
            "sync task group should not leak owned tasks after stop"
        );
    }

    #[tokio::test]
    async fn test_sync_manager_peer_management() {
        let manager = SyncServiceManager::with_defaults();

        let peer1 = DeviceId::new_from_entropy([1u8; 32]);
        let peer2 = DeviceId::new_from_entropy([2u8; 32]);

        // Add peers
        manager.add_peer(peer1).await;
        manager.add_peer(peer2).await;

        let peers = manager.peers().await;
        assert_eq!(peers.len(), 2);
        assert!(peers.contains(&peer1));
        assert!(peers.contains(&peer2));

        // Remove peer
        manager.remove_peer(&peer1).await;
        let peers = manager.peers().await;
        assert_eq!(peers.len(), 1);
        assert!(!peers.contains(&peer1));
        assert!(peers.contains(&peer2));
    }

    #[tokio::test]
    async fn test_sync_manager_health_when_not_running() {
        let manager = SyncServiceManager::with_defaults();

        // Health should be None when not running
        assert!(manager.sync_service_health().await.is_none());
    }

    #[tokio::test]
    async fn test_sync_manager_health_when_running() {
        let manager = SyncServiceManager::new(SyncManagerConfig::for_testing());
        let context = test_service_context().await;

        RuntimeService::start(&manager, &context).await.unwrap();

        // Health should be available when running
        let health = manager.sync_service_health().await;
        assert!(health.is_some());

        RuntimeService::stop(&manager).await.unwrap();
    }

    #[tokio::test]
    async fn test_sync_manager_without_merkle_verification() {
        let manager = SyncServiceManager::new(SyncManagerConfig::for_testing());

        // Manager without indexed journal should not have Merkle verification
        assert!(!manager.has_merkle_verification());
        assert!(manager.local_merkle_root().await.is_none());
        assert!(manager.verify_facts(vec![], [0u8; 32]).await.is_none());
        assert!(manager.merkle_verifier().is_none());
    }

    #[tokio::test]
    async fn test_sync_manager_with_merkle_verification() {
        let root = [42u8; 32];
        let journal = Arc::new(MockIndexedJournal::new(root));
        let time = Arc::new(PhysicalTimeHandler::new());
        let manager = SyncServiceManager::with_indexed_journal(
            SyncManagerConfig::for_testing(),
            journal,
            time,
        );

        // Manager with indexed journal should have Merkle verification
        assert!(manager.has_merkle_verification());
        assert!(manager.merkle_verifier().is_some());

        // Should return local Merkle root
        let local_root = manager.local_merkle_root().await;
        assert_eq!(local_root, Some(root));
    }

    #[tokio::test]
    async fn test_sync_manager_verify_facts() {
        let root = [42u8; 32];
        let journal = Arc::new(MockIndexedJournal::new(root));
        let time = Arc::new(PhysicalTimeHandler::new());
        let manager = SyncServiceManager::with_indexed_journal(
            SyncManagerConfig::for_testing(),
            journal,
            time,
        );

        // Create test fact with required authority
        let fact = IndexedFact {
            id: FactId(1),
            predicate: "test".to_string(),
            value: FactValue::String("test_value".to_string()),
            authority: Some(AuthorityId::new_from_entropy([1u8; 32])),
            timestamp: None,
        };

        // Legacy proofless fact verification is fail-closed.
        let result = manager.verify_facts(vec![fact], root).await;
        assert!(result.is_none());
    }
    #[test]
    fn required_sync_authorization_backoff_uses_typed_original_source_only() {
        let denial = aura_core::AuraError::PermissionDenied {
            message: "actual domain denial".into(),
            source: None,
        };
        assert!(sync_authorization_denial(&denial));
        let nested = aura_core::AuraError::Internal {
            message: "operation wrapper".into(),
            source: Some(Arc::new(denial)),
        };
        assert!(sync_authorization_denial(&nested));
        let diagnostic = aura_core::AuraError::Internal {
            message: "Permission denied; Authorization required".into(),
            source: None,
        };
        assert!(
            !sync_authorization_denial(&diagnostic),
            "diagnostic text cannot authorize retry policy"
        );
    }

    #[tokio::test]
    async fn required_manual_sync_clock_failure_retains_original_source_without_time_zero() {
        use std::error::Error;
        let clock = aura_testkit::time::ManualPhysicalClock::new(1000);
        let effects =
            crate::testing::simulation_effect_system(&crate::core::AgentConfig::default())
                .with_physical_time_provider(Arc::new(clock.clone()));
        let tasks = Arc::new(crate::runtime::TaskSupervisor::new());
        let context = RuntimeServiceContext::test_original(tasks, Arc::new(clock.clone())).await;
        let mut config = SyncManagerConfig::manual_only();
        config.maintenance_enabled = false;
        let manager = SyncServiceManager::with_indexed_journal(
            config,
            effects.indexed_journal(),
            Arc::new(clock.clone()),
        );
        manager
            .start(&context)
            .await
            .expect("actual owned manager starts before injected clock failure");
        clock
            .fail_next_observation(aura_core::effects::time::TimeError::OperationFailed {
                reason: "actual manual sync observation fault".into(),
            })
            .await;
        let failure = manager
            .sync_with_peers(&effects, vec![DeviceId::new_from_entropy([69; 32])])
            .await
            .expect_err("required clock failure cannot fabricate a peer-sync start timestamp");
        assert!(matches!(&failure, SyncManagerError::RequiredClock(_)));
        assert!(
            matches!(failure.source().and_then(|cause| cause.downcast_ref::<aura_core::effects::time::TimeError>()), Some(aura_core::effects::time::TimeError::OperationFailed { reason }) if reason == "actual manual sync observation fault")
        );
        manager
            .stop()
            .await
            .expect("actual started manager cleanup acknowledges command actor completion");
    }
}
