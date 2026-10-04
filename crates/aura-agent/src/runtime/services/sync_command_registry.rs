//! Runtime-held sync command actors, including cancelled foreground commands.
use super::{
    RuntimeService, RuntimeServiceContext, ServiceError, ServiceHealth, SyncManagerConfig,
    SyncServiceManager,
};
use crate::runtime::system::{
    RuntimeOperationLease, RuntimeShutdownOwnerReference, RuntimeShutdownWindowCapability,
};
use crate::runtime::{AuraEffectSystem, TaskSupervisor};
use async_trait::async_trait;
use aura_core::effects::PhysicalTimeEffects;
use aura_core::time::timeout::execute_with_timeout_budget;
use aura_core::{AuraError, DeviceId, TimeoutBudget};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{watch, Mutex};

const MAX_SYNC_COMMANDS: usize = 16;

#[derive(Debug, thiserror::Error)]
pub(crate) enum SyncCommandOwnershipError {
    #[error("runtime sync command admission is closed")]
    Closed,
    #[error("runtime sync command registry capacity is exhausted")]
    Capacity,
    #[error("sync registry stop requires original runtime shutdown custody")]
    ShutdownOwnerRequired,
    #[error("sync command belongs to another runtime registry")]
    ForeignOwner,
    #[error("sync registry completion belongs to another original shutdown")]
    ForeignShutdown,
}

struct RegisteredSyncCommand {
    manager: SyncServiceManager,
    context: RuntimeServiceContext,
    stopped: watch::Sender<bool>,
    lifecycle: Mutex<()>,
    rounds: crate::runtime::TaskGroup,
}
struct RegistryState {
    entries: Vec<Arc<RegisteredSyncCommand>>,
    closed: bool,
    acknowledged_shutdown: Option<RuntimeShutdownOwnerReference>,
}

/// Actual runtime service owner retains command actors after frontend cancellation.
#[derive(Clone)]
#[aura_macros::actor_root(
    owner = "sync_command_registry",
    domain = "runtime_sync_commands",
    supervision = "runtime_task_root",
    category = "actor_owned"
)]
pub(crate) struct SyncCommandRegistryService {
    effects: Arc<AuraEffectSystem>,
    tasks: Arc<TaskSupervisor>,
    state: Arc<Mutex<RegistryState>>,
}
impl SyncCommandRegistryService {
    pub(crate) fn new(effects: Arc<AuraEffectSystem>, tasks: Arc<TaskSupervisor>) -> Self {
        Self {
            effects,
            tasks,
            state: Arc::new(Mutex::new(RegistryState {
                entries: Vec::new(),
                closed: false,
                acknowledged_shutdown: None,
            })),
        }
    }

    #[aura_macros::capability_boundary(category = "capability_gated", capability = "RuntimeOperationLease", capability_type = RuntimeOperationLease, family = "runtime_helper")]
    pub(crate) async fn start_registered(
        &self,
        config: SyncManagerConfig,
        admission: RuntimeOperationLease,
    ) -> Result<AdmittedSyncCommandCapability, ServiceError> {
        let operation = self
            .effects
            .bound_admitted_runtime_operation(admission)
            .await
            .map_err(|source| {
                ServiceError::startup_failed(self.name(), "original admitted startup window failed")
                    .with_cause(source)
            })?;
        Box::pin(self.start_registered_owned(config, operation)).await
    }

    #[aura_macros::capability_boundary(category = "capability_gated", capability = "RuntimeBoundedOperationCapability", capability_type = crate::runtime::effects::RuntimeBoundedOperationCapability<'_>, family = "runtime_helper")]
    async fn start_registered_owned(
        &self,
        config: SyncManagerConfig,
        operation: crate::runtime::effects::RuntimeBoundedOperationCapability<'_>,
    ) -> Result<AdmittedSyncCommandCapability, ServiceError> {
        operation
            .require_runtime_owner(self.effects.as_ref())
            .map_err(|source| {
                ServiceError::startup_failed(self.name(), "foreign actual startup window owner")
                    .with_cause(source)
            })?;
        let window = operation.original_window().clone();
        let context = RuntimeServiceContext::new(
            self.tasks.clone(),
            Arc::new(self.effects.time_effects().clone()),
            window.clone(),
        );
        let auto_sync = config.auto_sync_enabled;
        let round_interval = config.auto_sync_interval;
        if auto_sync && round_interval.is_zero() {
            return Err(ServiceError::new(
                self.name(),
                super::ServiceErrorKind::InvalidConfiguration,
                "periodic sync interval must be positive",
            ));
        }
        let manager = SyncServiceManager::with_indexed_journal(
            config,
            self.effects.indexed_journal(),
            context.time_effects(),
        );
        let (stopped, _) = watch::channel(false);
        let entry = Arc::new(RegisteredSyncCommand {
            manager,
            context,
            stopped,
            lifecycle: Mutex::new(()),
            rounds: self.tasks.group("sync_command_periodic"),
        });
        let result = execute_with_timeout_budget(self.effects.as_ref(), &window, || async {
            {
                let mut state = self.state.lock().await;
                if state.closed {
                    return Err(
                        ServiceError::unavailable(self.name(), "command registry closed")
                            .with_cause(SyncCommandOwnershipError::Closed),
                    );
                }
                state.entries.retain(|entry| !*entry.stopped.borrow());
                if state.entries.len() >= MAX_SYNC_COMMANDS {
                    return Err(
                        ServiceError::unavailable(self.name(), "command registry full")
                            .with_cause(SyncCommandOwnershipError::Capacity),
                    );
                }
                state.entries.push(entry.clone());
            }
            // Registered resource custody survives cancellation before startup ACK.
            let _lifecycle = entry.lifecycle.lock().await;

            entry.manager.start(&entry.context).await?;
            entry.manager.required_running_health().await?;
            self.register_periodic_rounds(&entry, &operation, auto_sync, round_interval)?;
            if let Some(source) = entry.rounds.terminal_failure() {
                return Err(ServiceError::startup_failed(
                    "sync_command",
                    "actual periodic task admission failed",
                )
                .with_cause(source));
            }

            Ok(())
        })
        .await
        .map_err(|source| super::traits::service_window_failure(self.name(), source));
        if let Err(primary) = result {
            let command = AdmittedSyncCommandCapability {
                registry: self.clone(),
                entry: entry.clone(),
            };
            if let Err(cleanup) = command.stop().await {
                return Err(ServiceError::startup_failed(
                    self.name(),
                    "sync startup and required partial cleanup failed",
                )
                .with_cause(SyncCommandStartupCleanupFailure { primary, cleanup }));
            }
            return Err(primary);
        }
        // Hand off into the registry's actual runtime-owned service lifetime.
        // A daemon holds no admission lease while waiting for Ctrl+C.
        drop(operation);
        Ok(AdmittedSyncCommandCapability {
            registry: self.clone(),
            entry,
        })
    }

    #[aura_macros::capability_boundary(category = "capability_gated", capability = "RuntimeBoundedOperationCapability", capability_type = crate::runtime::effects::RuntimeBoundedOperationCapability<'_>, family = "runtime_helper")]
    fn register_periodic_rounds(
        &self,
        entry: &Arc<RegisteredSyncCommand>,
        operation: &crate::runtime::effects::RuntimeBoundedOperationCapability<'_>,
        auto_sync: bool,
        round_interval: Duration,
    ) -> Result<(), ServiceError> {
        operation
            .require_runtime_owner(self.effects.as_ref())
            .map_err(|source| {
                ServiceError::startup_failed(self.name(), "foreign periodic registration owner")
                    .with_cause(source)
            })?;
        if !Arc::ptr_eq(&entry.context.tasks(), &self.tasks)
            || !self.tasks.owns_group(&entry.rounds)
        {
            return Err(
                ServiceError::startup_failed(self.name(), "foreign periodic task owner")
                    .with_cause(SyncCommandOwnershipError::ForeignOwner),
            );
        }
        if auto_sync {
            let registry = self.clone();
            let retained = entry.clone();
            let _owned_round = entry.rounds.spawn_try_interval_until_named(
                "sync.command.periodic",
                entry.context.time_effects(),
                round_interval,
                move || {
                    let command = AdmittedSyncCommandCapability {
                        registry: registry.clone(),
                        entry: retained.clone(),
                    };
                    async move {
                        match command.sync_request(None).await {
                            Ok(_) => Ok(true),
                            Err(source) if original_command_closed(&source) => Ok(false),
                            Err(source) => Err(source),
                        }
                    }
                },
            );
        }
        Ok(())
    }

    pub(in crate::runtime) async fn stop_with_original_shutdown(
        &self,
        original: &RuntimeShutdownWindowCapability,
    ) -> Result<(), ServiceError> {
        original
            .require_service_owner(self.effects.as_ref(), &self.tasks)
            .map_err(|source| {
                ServiceError::shutdown_failed(self.name(), "foreign runtime shutdown owner")
                    .with_cause(source)
            })?;
        let entries = {
            let mut state = original
                .execute_service(self.effects.as_ref(), &self.tasks, self.name(), || async {
                    Ok(self.state.lock().await)
                })
                .await?;
            if let Some(ack) = &state.acknowledged_shutdown {
                if !original.same_owner(ack) {
                    return Err(ServiceError::shutdown_failed(
                        self.name(),
                        "foreign original stop acknowledgement",
                    )
                    .with_cause(SyncCommandOwnershipError::ForeignShutdown));
                }
                return Ok(());
            }
            state.closed = true;
            state.entries.clone()
        };
        for entry in entries {
            original
                .execute_service(self.effects.as_ref(), &self.tasks, self.name(), || async {
                    entry.rounds.request_cancellation();
                    let _lifecycle = entry.lifecycle.lock().await;
                    let command = AdmittedSyncCommandCapability {
                        registry: self.clone(),
                        entry: entry.clone(),
                    };
                    let stop = SyncCommandStopCapability {
                        command: &command,
                        scope: SyncCommandStopScope::Runtime(original),
                    };
                    command.stop_registered_resources(&stop).await?;
                    if !matches!(entry.manager.health().await, ServiceHealth::Stopped) {
                        return Err(ServiceError::shutdown_failed(
                            self.name(),
                            "sync command did not acknowledge stopped state",
                        ));
                    }
                    Ok(())
                })
                .await?;
            original
                .acknowledge_service_progress(
                    self.effects.as_ref(),
                    &self.tasks,
                    self.name(),
                    || {
                        entry.stopped.send_replace(true);
                    },
                )
                .await?;
        }
        let mut state = original
            .execute_service(self.effects.as_ref(), &self.tasks, self.name(), || async {
                Ok(self.state.lock().await)
            })
            .await?;
        original
            .acknowledge_service_progress(self.effects.as_ref(), &self.tasks, self.name(), || {
                state.entries.clear();
                state.acknowledged_shutdown = Some(original.owner_reference());
            })
            .await
    }
}

#[async_trait]
impl RuntimeService for SyncCommandRegistryService {
    fn name(&self) -> &'static str {
        "sync_command_registry"
    }
    fn dependencies(&self) -> &[&'static str] {
        &["indexed_journal", "transport"]
    }
    async fn start(&self, _context: &RuntimeServiceContext) -> Result<(), ServiceError> {
        Ok(())
    }
    async fn stop(&self) -> Result<(), ServiceError> {
        // Replay only this actual registry's completed original-owner ACK.
        // Observed health/closed flags and task cancellation are not proof.
        if self.state.lock().await.acknowledged_shutdown.is_some() {
            return Ok(());
        }
        Err(
            ServiceError::shutdown_failed(self.name(), "original shutdown owner required")
                .with_cause(SyncCommandOwnershipError::ShutdownOwnerRequired),
        )
    }
    async fn health(&self) -> ServiceHealth {
        let entries = {
            let state = self.state.lock().await;
            if state.acknowledged_shutdown.is_some() {
                return ServiceHealth::Stopped;
            }
            if state.closed {
                return ServiceHealth::Unhealthy {
                    reason: "required command teardown is incomplete".into(),
                };
            }
            state.entries.clone()
        };
        for entry in entries {
            if *entry.stopped.borrow() {
                continue;
            }
            if let Some(source) = entry.rounds.terminal_failure() {
                return ServiceHealth::Unhealthy {
                    reason: source.to_string(),
                };
            }
            if let Err(source) = entry.manager.required_running_health().await {
                return ServiceHealth::Unhealthy {
                    reason: source.to_string(),
                };
            }
        }
        ServiceHealth::Healthy
    }
}

/// Exact private command and original cleanup scope, never a raw manager/id.
pub(crate) struct SyncCommandStopCapability<'a> {
    command: &'a AdmittedSyncCommandCapability,
    scope: SyncCommandStopScope<'a>,
}
enum SyncCommandStopScope<'a> {
    Runtime(&'a RuntimeShutdownWindowCapability),
    Command(&'a TimeoutBudget),
}
impl SyncCommandStopCapability<'_> {
    pub(crate) fn require_manager(&self, manager: &SyncServiceManager) -> Result<(), ServiceError> {
        if !manager.shares_owner(&self.command.entry.manager) {
            return Err(ServiceError::shutdown_failed(
                "sync_command",
                "foreign manager stop owner",
            )
            .with_cause(SyncCommandOwnershipError::ForeignOwner));
        }
        Ok(())
    }
    pub(crate) async fn shutdown_tasks(
        &self,
        group: &crate::runtime::TaskGroup,
    ) -> Result<(), ServiceError> {
        let registry = &self.command.registry;
        match self.scope {
            SyncCommandStopScope::Runtime(original) => {
                original
                    .shutdown_service_tasks(
                        registry.effects.as_ref(),
                        &registry.tasks,
                        "sync_command",
                        group,
                    )
                    .await
            }
            SyncCommandStopScope::Command(original) => {
                if !registry.tasks.owns_group(group) {
                    return Err(ServiceError::shutdown_failed(
                        "sync_command",
                        "foreign command task group",
                    )
                    .with_cause(SyncCommandOwnershipError::ForeignOwner));
                }
                group
                    .shutdown_with_original_budget(registry.effects.as_ref(), original)
                    .await
                    .map_err(|source| {
                        ServiceError::shutdown_failed(
                            "sync_command",
                            "required original command worker completion failed",
                        )
                        .with_cause(source)
                    })
            }
        }
    }
}

/// Retains one actual registered command without holding startup admission open.
/// Construction, cloning, context extraction and serialization are unavailable.
///
/// ```compile_fail
/// fn duplicate(command: &aura_agent::AdmittedSyncCommandCapability) {
///     let _: aura_agent::AdmittedSyncCommandCapability = command.clone();
/// }
/// ```
///
/// ```compile_fail
/// fn detach(command: &aura_agent::AdmittedSyncCommandCapability) {
///     let _manager = command.manager();
/// }
/// ```
#[must_use = "retain command control until explicit stop or runtime-owned shutdown"]
pub struct AdmittedSyncCommandCapability {
    registry: SyncCommandRegistryService,
    entry: Arc<RegisteredSyncCommand>,
}
impl AdmittedSyncCommandCapability {
    pub async fn health(&self) -> ServiceHealth {
        if let Some(source) = self.entry.rounds.terminal_failure() {
            return ServiceHealth::Unhealthy {
                reason: source.to_string(),
            };
        }
        self.entry.manager.health().await
    }
    pub async fn metrics(&self) -> Option<aura_sync::services::ServiceMetrics> {
        self.entry.manager.metrics().await
    }
    pub async fn peers(&self) -> Vec<DeviceId> {
        self.entry.manager.peers().await
    }
    pub fn time_effects(&self) -> Arc<dyn PhysicalTimeEffects + Send + Sync> {
        self.entry.context.time_effects()
    }
    pub fn terminal_failure(&self) -> Option<crate::task_registry::TaskSupervisionError> {
        self.registry.tasks.terminal_failure()
    }
    pub async fn closed(&self) {
        let mut stopped = self.entry.stopped.subscribe();
        while !*stopped.borrow_and_update() {
            if stopped.changed().await.is_err() {
                break;
            }
        }
    }
    /// Perform the real protocol for every requested peer under one original
    /// admitted resource window. Empty input performs no requested work.
    pub async fn sync_with_peers(&self, peers: Vec<DeviceId>) -> Result<(), AuraError> {
        self.sync_request(Some(peers)).await.map(|_| ())
    }
    async fn sync_request(
        &self,
        requested: Option<Vec<DeviceId>>,
    ) -> Result<SyncRoundDisposition, AuraError> {
        if *self.entry.stopped.borrow() {
            return Err(source_error(SyncCommandOwnershipError::Closed));
        }
        let operation = self
            .registry
            .effects
            .admit_bounded_runtime_operation()
            .await
            .map_err(agent_source)?;
        operation
            .execute(|| async {
                let _lifecycle = self.entry.lifecycle.lock().await;
                if *self.entry.stopped.borrow() {
                    return Err(crate::core::AgentError::from(source_error(
                        SyncCommandOwnershipError::Closed,
                    )));
                }
                let peers = match requested {
                    Some(peers) => peers,
                    None => self
                        .entry
                        .manager
                        .required_tracked_peers()
                        .await
                        .map_err(|source| crate::core::AgentError::from(source_error(source)))?,
                };
                if peers.is_empty() {
                    return Ok(SyncRoundDisposition::NoTrackedPeers);
                }
                self.entry
                    .manager
                    .ensure_biscuit_authorization(self.registry.effects.as_ref())
                    .await
                    .map_err(|source| crate::core::AgentError::from(source_error(source)))?;
                self.entry
                    .manager
                    .sync_requested_peers_in_original_window(
                        self.registry.effects.as_ref(),
                        peers,
                        operation.original_window(),
                    )
                    .await
                    .map_err(|source| crate::core::AgentError::from(source_error(source)))?;
                Ok(SyncRoundDisposition::Completed)
            })
            .await
            .map_err(agent_source)
    }
    #[aura_macros::capability_boundary(category = "capability_gated", capability = "SyncCommandStopCapability", capability_type = SyncCommandStopCapability<'_>, family = "runtime_helper")]
    async fn stop_registered_resources(
        &self,
        original: &SyncCommandStopCapability<'_>,
    ) -> Result<(), ServiceError> {
        if !std::ptr::eq(self, original.command) {
            return Err(ServiceError::shutdown_failed(
                "sync_command",
                "foreign exact command stop owner",
            )
            .with_cause(SyncCommandOwnershipError::ForeignOwner));
        }
        let manager = self.entry.manager.stop_registered_command(original).await;
        let rounds = original.shutdown_tasks(&self.entry.rounds).await;
        match (manager, rounds) {
            (Ok(()), Ok(())) => Ok(()),
            (Err(source), Ok(())) | (Ok(()), Err(source)) => Err(source),
            (Err(primary), Err(cleanup)) => Err(ServiceError::shutdown_failed(
                "sync_command",
                "manager and periodic task retirement both failed",
            )
            .with_cause(SyncCommandStopFailures { primary, cleanup })),
        }
    }

    pub async fn stop(&self) -> Result<(), ServiceError> {
        let started = self
            .registry
            .effects
            .physical_time()
            .await
            .map_err(|source| {
                ServiceError::unavailable("sync_command", "required command cleanup clock failed")
                    .with_cause(source)
            })?;
        let cleanup = TimeoutBudget::from_start_and_timeout(&started, Duration::from_secs(5))
            .map_err(|source| {
                ServiceError::shutdown_failed("sync_command", "invalid command cleanup window")
                    .with_cause(source)
            })?;
        execute_with_timeout_budget(self.registry.effects.as_ref(), &cleanup, || async {
            self.entry.rounds.request_cancellation();
            let _lifecycle = self.entry.lifecycle.lock().await;
            if *self.entry.stopped.borrow() {
                return Ok(());
            }
            let stop = SyncCommandStopCapability {
                command: self,
                scope: SyncCommandStopScope::Command(&cleanup),
            };
            self.stop_registered_resources(&stop).await?;
            if !matches!(self.entry.manager.health().await, ServiceHealth::Stopped) {
                return Err(ServiceError::shutdown_failed(
                    "sync_command",
                    "command did not acknowledge stopped state",
                ));
            }
            self.entry.stopped.send_replace(true);
            Ok(())
        })
        .await
        .map_err(|source| super::traits::service_window_failure("sync_command", source))
    }
}
#[derive(Debug, thiserror::Error)]
#[error("{primary}; sync cleanup also failed: {cleanup}")]
struct SyncCommandStartupCleanupFailure {
    #[source]
    primary: ServiceError,
    cleanup: ServiceError,
}

fn source_error(source: impl std::error::Error + Send + Sync + 'static) -> AuraError {
    AuraError::Internal {
        message: "required owned sync command failed".into(),
        source: Some(Arc::new(source)),
    }
}
fn agent_source(source: crate::core::AgentError) -> AuraError {
    match source {
        crate::core::AgentError::Aura(source) => source,
        source => source_error(source),
    }
}

#[derive(Debug)]
enum SyncRoundDisposition {
    NoTrackedPeers,
    Completed,
}

fn original_command_closed(error: &AuraError) -> bool {
    let mut source: Option<&(dyn std::error::Error + 'static)> = Some(error);
    while let Some(original) = source {
        if matches!(
            original.downcast_ref::<SyncCommandOwnershipError>(),
            Some(SyncCommandOwnershipError::Closed)
        ) || matches!(
            original.downcast_ref::<crate::runtime::system::RuntimePublicOperationError>(),
            Some(crate::runtime::system::RuntimePublicOperationError::NotAccepting { .. })
        ) {
            return true;
        }
        source = original.source();
    }
    false
}
#[derive(Debug, thiserror::Error)]
#[error("{primary}; periodic sync retirement also failed: {cleanup}")]
struct SyncCommandStopFailures {
    #[source]
    primary: ServiceError,
    cleanup: ServiceError,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::error::Error;

    struct ObservedPeriodicSleep {
        clock: aura_testkit::time::ManualPhysicalClock,
        periodic_ingress: watch::Sender<bool>,
    }
    #[async_trait::async_trait]
    impl PhysicalTimeEffects for ObservedPeriodicSleep {
        async fn physical_time(
            &self,
        ) -> Result<aura_core::time::PhysicalTime, aura_core::effects::time::TimeError> {
            self.clock.physical_time().await
        }
        async fn sleep_ms(&self, duration: u64) -> Result<(), aura_core::effects::time::TimeError> {
            let sleep = self.clock.sleep_ms(duration);
            tokio::pin!(sleep);
            futures::future::poll_fn(|context| {
                let progress = std::future::Future::poll(sleep.as_mut(), context);
                if duration == 10 && progress.is_pending() {
                    // The actual provider has captured its target and enabled
                    // its wake registration before exposing this test witness.
                    self.periodic_ingress.send_replace(true);
                }
                progress
            })
            .await
        }
    }

    fn original_source<'a, T: Error + 'static>(error: &'a (dyn Error + 'static)) -> Option<&'a T> {
        let mut cause = Some(error);
        while let Some(actual) = cause {
            if let Some(typed) = actual.downcast_ref::<T>() {
                return Some(typed);
            }
            cause = actual.source();
        }
        None
    }

    fn required_transport_fault_fixture(
        label: &'static str,
    ) -> (
        AuraEffectSystem,
        Arc<aura_testkit::stateful_effects::custom_provider::CustomProviderProbe>,
    ) {
        use aura_testkit::stateful_effects::custom_provider::CustomProviderProbe;
        let resources = Arc::new(CustomProviderProbe::default());
        let transport = Arc::new(CustomProviderProbe::default());
        transport.set_ready(true);
        let effects = AuraEffectSystem::custom_for_authority(
            crate::core::AgentConfig::default(),
            aura_core::AuthorityId::new_from_entropy(aura_core::hash::hash(label.as_bytes())),
            aura_core::effects::ExecutionMode::Testing,
            crate::runtime::effects::SelectedCustomProviders {
                crypto: Arc::new(aura_effects::crypto::RealCryptoHandler::new()),
                storage: resources.clone(),
                random: resources.clone(),
                console: resources,
                transports: vec![transport.clone()],
            },
            None,
            None,
        )
        .expect("actual configured nonfaulting providers and selected transport");
        (effects, transport)
    }

    #[test]
    fn required_sync_registry_start_caller_frame_stays_bounded_before_first_poll() {
        let effects =
            crate::testing::simulation_effect_system_arc(&crate::core::AgentConfig::default());
        let registry =
            SyncCommandRegistryService::new(effects.clone(), Arc::new(TaskSupervisor::new()));
        let admission = effects
            .admit_public_operation()
            .expect("actual unpolled ingress owner");
        let startup = registry.start_registered(SyncManagerConfig::manual_only(), admission);
        let bytes = std::mem::size_of_val(&startup);
        assert!(
            bytes <= 16 * 1024,
            "actual command start caller frame is {bytes} bytes"
        );
        drop(startup);
        assert!(registry
            .state
            .try_lock()
            .expect("unpolled startup did not lock registry")
            .entries
            .is_empty());
    }

    #[tokio::test]
    async fn required_sync_registry_rejects_foreign_actual_admission_before_birth() {
        let effects =
            crate::testing::simulation_effect_system_arc(&crate::core::AgentConfig::default());
        let foreign =
            crate::testing::simulation_effect_system_arc(&crate::core::AgentConfig::default());
        let registry = SyncCommandRegistryService::new(effects, Arc::new(TaskSupervisor::new()));
        let admission = foreign
            .admit_public_operation()
            .expect("actual foreign admitted owner");
        let failure = match registry
            .start_registered(SyncManagerConfig::manual_only(), admission)
            .await
        {
            Ok(_) => panic!("foreign gate must not issue command custody"),
            Err(failure) => failure,
        };
        assert!(
            original_source::<crate::runtime::system::RuntimePublicOperationError>(&failure)
                .is_some()
        );
        assert!(
            registry.state.lock().await.entries.is_empty(),
            "no command is born after foreign admission"
        );
    }

    #[tokio::test]
    async fn required_sync_registered_lifetime_releases_start_admission_and_acknowledges_stop() {
        let effects =
            crate::testing::simulation_effect_system_arc(&crate::core::AgentConfig::default());
        let tasks = Arc::new(TaskSupervisor::new());
        let registry = SyncCommandRegistryService::new(effects.clone(), tasks);
        let missing_ack = registry
            .stop()
            .await
            .expect_err("observed healthy registry is not an original stop ACK");
        assert!(matches!(
            original_source::<SyncCommandOwnershipError>(&missing_ack),
            Some(SyncCommandOwnershipError::ShutdownOwnerRequired)
        ));
        let admission = effects
            .admit_public_operation()
            .expect("actual command ingress owner");
        let mut config = SyncManagerConfig::manual_only();
        config.maintenance_enabled = false;
        let command = registry
            .start_registered(config, admission)
            .await
            .expect("actual registered actor startup");
        assert!(matches!(
            command.health().await,
            ServiceHealth::Healthy | ServiceHealth::Degraded { .. }
        ));
        assert_eq!(registry.state.lock().await.entries.len(), 1);
        let gate = effects.public_operation_activity();
        let mut drain = Box::pin(gate.wait_for_operations());
        assert!(
            futures::poll!(drain.as_mut()).is_ready(),
            "registered actor lifetime retains no ordinary startup admission"
        );
        drop(drain);
        command
            .stop()
            .await
            .expect("required actual command/service/task completion");
        assert!(*command.entry.stopped.borrow());
        assert!(matches!(command.health().await, ServiceHealth::Stopped));
        let missing_ack = registry.stop().await.expect_err(
            "individual command completion is not a whole-registry original shutdown ACK",
        );
        assert!(matches!(
            original_source::<SyncCommandOwnershipError>(&missing_ack),
            Some(SyncCommandOwnershipError::ShutdownOwnerRequired)
        ));
    }
    #[tokio::test]
    async fn required_sync_command_returns_actual_peer_protocol_refusal_before_success() {
        let (effects, transport) =
            required_transport_fault_fixture("required-sync-command-native-send-fault");
        let effects = Arc::new(effects);
        let registry =
            SyncCommandRegistryService::new(effects.clone(), Arc::new(TaskSupervisor::new()));
        let admission = effects
            .admit_public_operation()
            .expect("actual request ingress");
        let mut config = SyncManagerConfig::manual_only();
        config.maintenance_enabled = false;
        // Exercise one actual provider attempt; no simulated callback/error.
        config.journal_sync.anti_entropy.retry_enabled = false;
        let command = registry
            .start_registered(config, admission)
            .await
            .expect("actual registered manual actor");
        transport.set_fault(true);
        let peer = DeviceId::new_from_entropy([83; 32]);
        let failure = command
            .sync_with_peers(vec![peer])
            .await
            .expect_err("actual configured send failure cannot report requested peer success");
        assert!(
            matches!(original_source::<aura_sync::services::RequiredPeerSyncError>(&failure),
            Some(aura_sync::services::RequiredPeerSyncError::Protocol { peer: refused, .. }) if *refused == peer)
        );
        assert!(
            matches!(
                original_source::<aura_core::effects::transport::TransportError>(&failure),
                Some(aura_core::effects::transport::TransportError::DestinationUnreachable { .. })
            ),
            "actual selected native send failure must remain in the source chain: {failure:?}"
        );
        assert_eq!(
            transport.sends(),
            1,
            "actual configured single-attempt protocol policy reaches its producer"
        );
        command
            .stop()
            .await
            .expect("failed request still performs real actor/service cleanup");
        assert!(*command.entry.stopped.borrow());
    }
    #[tokio::test]
    async fn required_sync_idle_round_does_not_claim_peer_completion_or_keep_start_admission() {
        let effects =
            crate::testing::simulation_effect_system_arc(&crate::core::AgentConfig::default());
        let registry =
            SyncCommandRegistryService::new(effects.clone(), Arc::new(TaskSupervisor::new()));
        let mut config = SyncManagerConfig::manual_only();
        config.maintenance_enabled = false;
        let command = registry
            .start_registered(
                config,
                effects
                    .admit_public_operation()
                    .expect("original idle actor ingress"),
            )
            .await
            .expect("actual registered idle actor");
        assert!(matches!(
            command
                .sync_request(None)
                .await
                .expect("required actual tracked-peer query"),
            SyncRoundDisposition::NoTrackedPeers
        ));
        let gate = effects.public_operation_activity();
        let mut pending = Box::pin(gate.wait_for_operations());
        assert!(
            futures::poll!(pending.as_mut()).is_ready(),
            "completed idle round releases its own admitted operation"
        );
        drop(pending);
        command.stop().await.expect("actual idle actor stop ACK");
        let rejected = command
            .sync_request(None)
            .await
            .expect_err("stopped actor cannot fabricate later idle success");
        assert!(matches!(
            original_source::<SyncCommandOwnershipError>(&rejected),
            Some(SyncCommandOwnershipError::Closed)
        ));
    }
    #[tokio::test]
    async fn required_sync_start_clock_fault_precedes_actor_birth_and_releases_admission() {
        let clock = aura_testkit::time::ManualPhysicalClock::new(1000);
        let effects = Arc::new(
            crate::testing::simulation_effect_system(&crate::core::AgentConfig::default())
                .with_physical_time_provider(Arc::new(clock.clone())),
        );
        let registry =
            SyncCommandRegistryService::new(effects.clone(), Arc::new(TaskSupervisor::new()));
        let admission = effects
            .admit_public_operation()
            .expect("actual startup ingress");
        clock
            .fail_next_observation(aura_core::effects::time::TimeError::OperationFailed {
                reason: "actual command startup observation outage".into(),
            })
            .await;
        let failure = match registry
            .start_registered(SyncManagerConfig::manual_only(), admission)
            .await
        {
            Ok(_) => panic!("required startup clock failure cannot issue actor control"),
            Err(failure) => failure,
        };
        assert!(
            matches!(original_source::<aura_core::effects::time::TimeError>(&failure),
            Some(aura_core::effects::time::TimeError::OperationFailed { reason })
            if reason == "actual command startup observation outage")
        );
        assert!(
            registry.state.lock().await.entries.is_empty(),
            "required observation precedes any registered actor birth"
        );
        let gate = effects.public_operation_activity();
        let mut pending = Box::pin(gate.wait_for_operations());
        assert!(
            futures::poll!(pending.as_mut()).is_ready(),
            "failed startup consumes and releases its original admission"
        );
    }
    #[tokio::test]
    async fn required_sync_daemon_runs_real_tracked_peer_round_and_retains_task_fault() {
        let clock = aura_testkit::time::ManualPhysicalClock::new(1000);
        let (periodic_ingress, mut entered_periodic_sleep) = watch::channel(false);
        let provider = Arc::new(ObservedPeriodicSleep {
            clock: clock.clone(),
            periodic_ingress,
        });
        let (effects, transport) =
            required_transport_fault_fixture("required-sync-daemon-native-send-fault");
        let effects = Arc::new(effects.with_physical_time_provider(provider));
        let registry =
            SyncCommandRegistryService::new(effects.clone(), Arc::new(TaskSupervisor::new()));
        let mut config = SyncManagerConfig::manual_only();
        config.maintenance_enabled = false;
        // Exercise one actual provider attempt; no simulated callback/error.
        config.journal_sync.anti_entropy.retry_enabled = false;
        config.auto_sync_enabled = true;
        config.auto_sync_interval = Duration::from_millis(10);
        let command = registry
            .start_registered(
                config,
                effects
                    .admit_public_operation()
                    .expect("actual daemon startup admission"),
            )
            .await
            .expect("idle daemon starts with registered periodic owner");
        while !*entered_periodic_sleep.borrow_and_update() {
            entered_periodic_sleep
                .changed()
                .await
                .expect("actual periodic owner sleep ingress");
        }
        transport.set_fault(true);
        let peer = DeviceId::new_from_entropy([91; 32]);
        command.entry.manager.add_peer(peer).await;
        assert_eq!(
            command
                .entry
                .manager
                .required_tracked_peers()
                .await
                .expect("actual actor acknowledges tracked peer"),
            vec![peer]
        );
        // The TaskRegistry callback executes immediately once; advancing this
        // actual provider also releases a first idle callback's registered sleep.
        clock.set_time(1010);
        let now = clock
            .physical_time()
            .await
            .expect("actual drain observation");
        let original = TimeoutBudget::from_start_and_timeout(&now, Duration::from_secs(5))
            .expect("original test observation window");
        let failure = command
            .entry
            .rounds
            .wait_with_original_budget(effects.as_ref(), &original)
            .await
            .expect_err("real daemon peer refusal cannot be a successful idle tick");
        assert!(
            matches!(original_source::<aura_sync::services::RequiredPeerSyncError>(&failure),
            Some(aura_sync::services::RequiredPeerSyncError::Protocol { peer: refused, .. })
            if *refused == peer)
        );
        assert!(
            matches!(
                original_source::<aura_core::effects::transport::TransportError>(&failure),
                Some(aura_core::effects::transport::TransportError::DestinationUnreachable { .. })
            ),
            "actual daemon selected-send cause survives task supervision: {failure:?}"
        );
        assert_eq!(
            transport.sends(),
            1,
            "actual daemon uses configured single-attempt policy"
        );
        assert!(
            command.entry.rounds.terminal_failure().is_some(),
            "native protocol fault remains in actual owned task health"
        );
        assert!(
            matches!(command.health().await, ServiceHealth::Unhealthy { .. }),
            "actual failed round cannot report healthy command"
        );
        assert!(
            matches!(registry.health().await, ServiceHealth::Unhealthy { .. }),
            "actual failed round cannot report healthy registry"
        );
        let cleanup = command
            .stop()
            .await
            .expect_err("prior required task failure cannot become whole cleanup success");
        assert!(original_source::<crate::task_registry::TaskSupervisionError>(&cleanup).is_some());
        assert!(
            matches!(command.entry.manager.health().await, ServiceHealth::Stopped),
            "actual manager stop still acknowledges despite prior round fault"
        );
        assert!(
            !*command.entry.stopped.borrow(),
            "partial cleanup does not publish a complete command stop ACK"
        );
    }
    #[tokio::test]
    async fn required_sync_whole_shutdown_stops_registered_daemon_before_task_cancellation(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let profile = tempfile::tempdir()?;
        let authority = aura_core::AuthorityId::new_from_entropy(aura_core::hash::hash(
            b"work10-required-sync-whole-shutdown-authority",
        ));
        let context = crate::runtime::EffectContext::new(
            authority,
            aura_core::ContextId::new_from_entropy(aura_core::hash::hash(
                b"work10-required-sync-whole-shutdown-context",
            )),
            aura_core::effects::ExecutionMode::Testing,
        );
        let config = crate::AgentConfig {
            device_id: DeviceId::new_from_entropy(aura_core::hash::hash(
                b"work10-required-sync-whole-shutdown-device",
            )),
            storage: crate::core::config::StorageConfig {
                base_path: profile.path().to_path_buf(),
                ..Default::default()
            },
            ..Default::default()
        };
        let runtime = crate::runtime::builder::EffectSystemBuilder::testing()
            .with_config(config)
            .with_authority(authority)
            .build(&context)
            .await?;
        let mut command_config = SyncManagerConfig::manual_only();
        command_config.maintenance_enabled = false;
        command_config.auto_sync_enabled = true;
        command_config.auto_sync_interval = Duration::from_secs(60);
        let command = runtime.admit_sync_command(command_config).await?;
        assert!(
            !*command.entry.stopped.borrow(),
            "actual daemon is live before shutdown"
        );
        let activity = runtime.activity_gate();
        Box::pin(runtime.shutdown_typed(&context)).await?;
        assert!(
            *command.entry.stopped.borrow(),
            "required registered stop ACK precedes root disposal"
        );
        assert!(matches!(
            command.entry.manager.health().await,
            ServiceHealth::Stopped
        ));
        assert!(
            command.entry.rounds.terminal_failure().is_none(),
            "ordinary root cancellation does not replace acknowledged command cleanup"
        );
        assert!(matches!(
            activity.state(),
            crate::runtime::system::RuntimeActivityState::Stopped
        ));
        Ok(())
    }
}
