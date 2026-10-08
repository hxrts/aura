//! Background refresh-hook installation for system-owned derived state.

use super::refresh::{emit_chat_snapshot_signal, refresh_connection_status_from_contacts};
use crate::runtime_bridge::RuntimeBridge;
#[cfg(feature = "signals")]
use crate::signal_defs::{
    CHAT_SIGNAL, HOMES_SIGNAL, INVITATIONS_SIGNAL, RECOVERY_SIGNAL, TRANSPORT_PEERS_SIGNAL,
};
use crate::signal_defs::{CONTACTS_SIGNAL, SYNC_STATUS_SIGNAL};
use crate::workflows::runtime::workflow_best_effort;
use crate::{AppCore, ReactiveHandler};
use async_lock::RwLock;
use aura_core::effects::reactive::{ReactiveError, Signal, SignalStream};
use aura_core::{AuraError, OwnedTaskSpawner};
use futures::channel::oneshot;
use futures::future::{BoxFuture, Shared};
use futures::FutureExt;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

type HookCancellation = Shared<BoxFuture<'static, ()>>;

#[cfg(not(target_arch = "wasm32"))]
type BoxRefreshFuture = Pin<Box<dyn Future<Output = Result<(), AuraError>> + Send + 'static>>;
#[cfg(target_arch = "wasm32")]
type BoxRefreshFuture = Pin<Box<dyn Future<Output = Result<(), AuraError>> + 'static>>;
type RefreshHook = Arc<dyn Fn(Arc<RwLock<AppCore>>) -> BoxRefreshFuture + Send + Sync + 'static>;

#[derive(Debug, thiserror::Error)]
pub(crate) enum HookInstallError {
    #[error("runtime unavailable for refresh hooks")]
    RuntimeUnavailable,
    #[error("reactive attachment failed for {signal_id}: {source}")]
    Reactive {
        signal_id: String,
        #[source]
        source: ReactiveError,
    },
    #[error("refresh listener {name} did not start: {source}")]
    ListenerStart {
        name: &'static str,
        #[source]
        source: AuraError,
    },
    #[cfg(feature = "signals")]
    #[error("initial readiness refresh failed: {source}")]
    InitialRefresh {
        #[source]
        source: AuraError,
    },
    #[error("required hook attachment failed: {source}")]
    RequiredFailed {
        #[source]
        source: Arc<HookExecutionError>,
    },
    #[cfg(test)]
    #[error("injected hook attachment failure at step {step}")]
    Injected { step: usize },
}

/// Required refresh operation that failed within an owned attachment.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HookFailureStage {
    /// Receiving the authoritative signal failed.
    SignalReceive,
    /// Refreshing required state failed.
    Refresh,
    /// The required refresh interval provider failed.
    Interval,
}

/// First required failure retained by a runtime refresh attachment.
#[derive(Clone, Debug, thiserror::Error)]
#[error("refresh hook {name} failed during {stage:?}: {source}")]
pub struct HookExecutionError {
    name: &'static str,
    stage: HookFailureStage,
    #[source]
    source: AuraError,
}
impl HookExecutionError {
    /// Name of the failed owned listener.
    pub fn name(&self) -> &'static str {
        self.name
    }
    /// Structural stage of the original failure.
    pub fn stage(&self) -> HookFailureStage {
        self.stage
    }
    /// Original native failure, independent of tracing configuration.
    pub fn native_error(&self) -> &AuraError {
        &self.source
    }
}

struct HookHealth {
    first_failure: async_lock::Mutex<Option<Arc<HookExecutionError>>>,
    /// Most recent failed refresh of one signal update, per listener. It does
    /// not end the attachment: the listener keeps serving later updates.
    last_update_failures:
        async_lock::Mutex<std::collections::BTreeMap<&'static str, Arc<HookExecutionError>>>,
    cancelled: AtomicBool,
    abort: futures::future::AbortHandle,
}
impl HookHealth {
    fn new() -> (Arc<Self>, HookCancellation) {
        let (abort, registration) = futures::future::AbortHandle::new_pair();
        let cancellation =
            futures::future::Abortable::new(futures::future::pending::<()>(), registration)
                .map(|_| ())
                .boxed()
                .shared();
        (
            Arc::new(Self {
                first_failure: async_lock::Mutex::new(None),
                last_update_failures: async_lock::Mutex::new(std::collections::BTreeMap::new()),
                cancelled: AtomicBool::new(false),
                abort,
            }),
            cancellation,
        )
    }
    async fn fail(
        &self,
        name: &'static str,
        stage: HookFailureStage,
        source: AuraError,
    ) -> AuraError {
        let error = Arc::new(HookExecutionError {
            name,
            stage,
            source: source.clone(),
        });
        let mut first = self.first_failure.lock().await;
        if first.is_none() {
            *first = Some(error);
        }
        self.cancelled.store(true, Ordering::SeqCst);
        self.abort.abort();
        source
    }

    /// Retain the typed failure of one signal-driven refresh without
    /// cancelling the group; the next update re-reads current state.
    async fn record_update_failure(
        &self,
        name: &'static str,
        stage: HookFailureStage,
        source: AuraError,
    ) {
        #[cfg(feature = "instrumented")]
        tracing::warn!(
            hook = name,
            stage = ?stage,
            error = ?source,
            "refresh hook update failed; the listener continues with later updates"
        );
        self.last_update_failures.lock().await.insert(
            name,
            Arc::new(HookExecutionError {
                name,
                stage,
                source,
            }),
        );
    }
}

async fn refresh_chat_projection_and_readiness(
    app_core: &Arc<RwLock<AppCore>>,
) -> Result<(), AuraError> {
    let mut best_effort = workflow_best_effort();
    let _ = best_effort
        .capture(emit_chat_snapshot_signal(app_core))
        .await;
    #[cfg(feature = "signals")]
    {
        let _ = best_effort
            .capture(refresh_authoritative_channel_and_recipient_readiness_hook(
                app_core,
            ))
            .await;
    }
    best_effort.finish()
}

async fn refresh_contacts_and_readiness(app_core: &Arc<RwLock<AppCore>>) -> Result<(), AuraError> {
    let mut best_effort = workflow_best_effort();
    let _ = best_effort
        .capture(refresh_connection_status_from_contacts(app_core))
        .await;
    #[cfg(feature = "signals")]
    {
        let _ = best_effort
            .capture(refresh_authoritative_contact_link_readiness_hook(app_core))
            .await;
        let _ = best_effort
            .capture(crate::workflows::observed_projection::mirror_chat_signal_into_view(app_core))
            .await;
    }
    best_effort.finish()
}

#[cfg(feature = "signals")]
async fn refresh_authoritative_contact_link_readiness_hook(
    app_core: &Arc<RwLock<AppCore>>,
) -> Result<(), AuraError> {
    let mut best_effort = workflow_best_effort();
    let _ = best_effort
        .capture(crate::workflows::observed_projection::mirror_contacts_signal_into_view(app_core))
        .await;
    let _ = best_effort
        .capture(crate::workflows::observed_projection::mirror_homes_signal_into_view(app_core))
        .await;
    let _ = best_effort
        .capture(
            crate::workflows::invitation::refresh_authoritative_contact_link_readiness(app_core),
        )
        .await;
    best_effort.finish()
}

#[cfg(feature = "signals")]
async fn refresh_authoritative_invitation_and_channel_readiness_hook(
    app_core: &Arc<RwLock<AppCore>>,
) -> Result<(), AuraError> {
    let mut best_effort = workflow_best_effort();
    let _ = best_effort
        .capture(
            crate::workflows::observed_projection::mirror_invitations_signal_into_view(app_core),
        )
        .await;
    let _ = best_effort
        .capture(crate::workflows::invitation::refresh_authoritative_invitation_readiness(app_core))
        .await;
    let _ = best_effort
        .capture(
            crate::workflows::messaging::refresh_authoritative_channel_membership_readiness(
                app_core,
            ),
        )
        .await;
    let _ = best_effort
        .capture(
            crate::workflows::messaging::refresh_authoritative_recipient_resolution_readiness(
                app_core,
            ),
        )
        .await;
    best_effort.finish()
}

#[cfg(feature = "signals")]
async fn refresh_authoritative_channel_and_recipient_readiness_hook(
    app_core: &Arc<RwLock<AppCore>>,
) -> Result<(), AuraError> {
    let mut best_effort = workflow_best_effort();
    let _ = best_effort
        .capture(crate::workflows::invitation::refresh_authoritative_invitation_readiness(app_core))
        .await;
    let _ = best_effort
        .capture(
            crate::workflows::messaging::refresh_authoritative_channel_membership_readiness(
                app_core,
            ),
        )
        .await;
    let _ = best_effort
        .capture(
            crate::workflows::messaging::refresh_authoritative_recipient_resolution_readiness(
                app_core,
            ),
        )
        .await;
    best_effort.finish()
}

async fn spawn_owned_signal_refresh<T>(
    mut stream: SignalStream<T>,
    spawner: OwnedTaskSpawner,
    runtime: Arc<dyn RuntimeBridge>,
    app_core: Arc<RwLock<AppCore>>,
    refresh_name: &'static str,
    refresh: RefreshHook,
    attachment: (HookCancellation, Arc<HookHealth>),
) -> Result<(), HookInstallError>
where
    T: Clone + Send + Sync + 'static,
{
    let (cancel, health) = attachment;
    let (started_tx, started_rx) = oneshot::channel();

    spawn_cancellable_runtime_refresh_task(&spawner, refresh_name, async move {
        let _ = started_tx.send(());
        loop {
            let received = futures::select! {
                _ = cancel.clone().fuse() => break,
                received = stream.recv().fuse() => received,
            };
            if let Err(source) = received {
                let source = AuraError::Internal {
                    message: "required refresh signal failed".into(),
                    source: Some(Arc::new(source)),
                };
                return Err(health
                    .fail(refresh_name, HookFailureStage::SignalReceive, source)
                    .await);
            }

            // This task is the sole refresh owner. Updates received while the
            // refresh awaits remain in the bounded signal stream; after a lag,
            // recv resumes with a newer snapshot and refresh reads current state.
            let outcome = futures::select! {
                _ = cancel.clone().fuse() => break,
                outcome = refresh(app_core.clone()).fuse() => outcome,
            };
            // A failed refresh belongs to this update only. It is retained as a
            // typed per-update failure; later updates still refresh.
            if let Err(error) = outcome {
                health
                    .record_update_failure(refresh_name, HookFailureStage::Refresh, error)
                    .await;
            }
        }
        Ok(())
    })
    .map_err(|source| HookInstallError::ListenerStart {
        name: refresh_name,
        source,
    })?;
    crate::workflows::runtime::timeout_runtime_call(
        &runtime,
        "install_system_refresh_hooks",
        "listener_start",
        std::time::Duration::from_secs(5),
        || started_rx,
    )
    .await
    .map_err(|source| HookInstallError::ListenerStart {
        name: refresh_name,
        source,
    })?
    .map_err(|source| HookInstallError::ListenerStart {
        name: refresh_name,
        source: AuraError::Internal {
            message: "listener task exited before acknowledging startup".to_owned(),
            source: Some(Arc::new(source)),
        },
    })
}

async fn spawn_owned_enrollment_completion_refresh(
    spawner: OwnedTaskSpawner,
    runtime: Arc<dyn RuntimeBridge>,
    app_core: Arc<RwLock<AppCore>>,
    cancel: HookCancellation,
    health: Arc<HookHealth>,
) -> Result<(), HookInstallError> {
    let (started_tx, started_rx) = oneshot::channel();
    let startup_runtime = runtime.clone();
    spawn_cancellable_runtime_refresh_task(&spawner, "device_enrollment_completion_hook", async move {
        let _ = started_tx.send(());
        loop {
            let result = futures::select! {
                _ = cancel.clone().fuse() => break,
                result = crate::workflows::ceremonies::refresh_device_enrollment_completions(&app_core).fuse() => result,
            };
            if let Err(error) = result {
                return Err(health.fail("device_enrollment_completion_hook", HookFailureStage::Refresh, error).await);
            }
            match await_enrollment_refresh_interval(&runtime, cancel.clone()).await {
                Ok(true) => {}
                Ok(false) => break,
                Err(error) => {
                    return Err(health.fail("device_enrollment_completion_hook", HookFailureStage::Interval, error).await);
                }
            }
        }
        Ok(())
    }).map_err(|source| HookInstallError::ListenerStart { name: "device_enrollment_completion_hook", source })?;
    crate::workflows::runtime::timeout_runtime_call(
        &startup_runtime,
        "install_system_refresh_hooks",
        "enrollment_completion_start",
        std::time::Duration::from_secs(5),
        || started_rx,
    )
    .await
    .map_err(|source| HookInstallError::ListenerStart {
        name: "device_enrollment_completion_hook",
        source,
    })?
    .map_err(|source| HookInstallError::ListenerStart {
        name: "device_enrollment_completion_hook",
        source: AuraError::Internal {
            message: "enrollment completion task exited before acknowledging startup".to_owned(),
            source: Some(Arc::new(source)),
        },
    })
}

async fn await_enrollment_refresh_interval(
    runtime: &Arc<dyn RuntimeBridge>,
    cancel: HookCancellation,
) -> Result<bool, AuraError> {
    futures::select! {
        _ = cancel.fuse() => Ok(false),
        result = runtime.wait_for_background_refresh(1_000).fuse() => result.map(|_| true).map_err(|error| crate::workflows::error::runtime_call("enrollment refresh interval", error).into()),
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn spawn_cancellable_runtime_refresh_task<F>(
    spawner: &OwnedTaskSpawner,
    name: &'static str,
    fut: F,
) -> Result<(), AuraError>
where
    F: Future<Output = Result<(), AuraError>> + Send + 'static,
{
    spawner.spawn_fallible_cancellable(name, Box::pin(fut))
}

#[cfg(target_arch = "wasm32")]
fn spawn_cancellable_runtime_refresh_task<F>(
    spawner: &OwnedTaskSpawner,
    name: &'static str,
    fut: F,
) -> Result<(), AuraError>
where
    F: Future<Output = Result<(), AuraError>> + 'static,
{
    spawner.spawn_local_fallible_cancellable(name, Box::pin(fut))
}

/// Owns all refresh subscriptions for one runtime generation.
pub(crate) struct HookGroup {
    health: Arc<HookHealth>,
    #[cfg(test)]
    cancellation: HookCancellation,
    shutdown: aura_core::OwnedShutdownToken,
}

impl HookGroup {
    /// Serialize attachment-ready publication with the first required fault.
    pub(crate) async fn publish_ready(
        self,
        publish: impl FnOnce(Self),
    ) -> Result<(), Arc<HookExecutionError>> {
        let health = self.health.clone();
        let failure = health.first_failure.lock().await;
        if let Some(source) = failure.as_ref() {
            return Err(source.clone());
        }
        publish(self);
        Ok(())
    }

    pub(crate) async fn failure(&self) -> Option<Arc<HookExecutionError>> {
        self.health.first_failure.lock().await.clone()
    }

    /// Most recent per-update refresh failure of each listener; none of them
    /// ended the group.
    pub(crate) async fn update_failures(&self) -> Vec<Arc<HookExecutionError>> {
        self.health
            .last_update_failures
            .lock()
            .await
            .values()
            .cloned()
            .collect()
    }

    pub(crate) fn is_active(&self) -> bool {
        !self.health.cancelled.load(Ordering::SeqCst) && !self.shutdown.is_cancelled()
    }

    #[cfg(test)]
    pub(crate) fn cancellation_receiver(&self) -> HookCancellation {
        self.cancellation.clone()
    }
}

impl Drop for HookGroup {
    fn drop(&mut self) {
        self.health.cancelled.store(true, Ordering::SeqCst);
        self.health.abort.abort();
    }
}

struct AttachmentAttempt {
    next_step: usize,
    fail_at: Option<usize>,
}

impl AttachmentAttempt {
    async fn attach<T>(
        &mut self,
        reactive: &ReactiveHandler,
        signal: &Signal<T>,
    ) -> Result<SignalStream<T>, HookInstallError>
    where
        T: Clone + Send + Sync + 'static,
    {
        let step = self.next_step;
        self.next_step += 1;
        if self.fail_at == Some(step) {
            #[cfg(test)]
            return Err(HookInstallError::Injected { step });
            #[cfg(not(test))]
            unreachable!("attachment fault injection is test-only");
        }
        reactive
            .subscribe_attached(signal)
            .await
            .map_err(|source| HookInstallError::Reactive {
                signal_id: signal.id().to_string(),
                source,
            })
    }
}

/// Attach every graph receiver before any listener runs. An installation
/// failure drops the previously attached streams and leaves no live hook.
pub(crate) async fn install_system_refresh_hooks(
    app_core: &Arc<RwLock<AppCore>>,
) -> Result<HookGroup, HookInstallError> {
    install_system_refresh_hooks_with_fault(app_core, None).await
}

pub(crate) async fn install_system_refresh_hooks_with_fault(
    app_core: &Arc<RwLock<AppCore>>,
    fail_at: Option<usize>,
) -> Result<HookGroup, HookInstallError> {
    let (reactive, runtime) = {
        let core = app_core.read().await;
        (core.reactive().clone(), core.runtime().cloned())
    };
    let runtime = runtime.ok_or(HookInstallError::RuntimeUnavailable)?;
    let spawner = runtime.task_spawner();
    let mut attempt = AttachmentAttempt {
        next_step: 0,
        fail_at,
    };

    let contacts = attempt.attach(&reactive, &*CONTACTS_SIGNAL).await?;
    let chat = attempt.attach(&reactive, &*SYNC_STATUS_SIGNAL).await?;
    #[cfg(feature = "signals")]
    let chat_readiness = attempt.attach(&reactive, &*CHAT_SIGNAL).await?;
    #[cfg(feature = "signals")]
    let homes_readiness = attempt.attach(&reactive, &*HOMES_SIGNAL).await?;
    #[cfg(feature = "signals")]
    let peers_readiness = attempt.attach(&reactive, &*TRANSPORT_PEERS_SIGNAL).await?;
    #[cfg(feature = "signals")]
    let invitation_readiness = attempt.attach(&reactive, &*INVITATIONS_SIGNAL).await?;
    #[cfg(feature = "signals")]
    let recovery_projection = attempt.attach(&reactive, &*RECOVERY_SIGNAL).await?;

    let (health, cancel_rx) = HookHealth::new();
    let group = HookGroup {
        health: health.clone(),
        #[cfg(test)]
        cancellation: cancel_rx.clone(),
        shutdown: spawner.shutdown_token().clone(),
    };
    let enrollment_spawner = spawner.clone();
    let enrollment_runtime = runtime.clone();
    let enrollment_cancel = cancel_rx.clone();
    spawn_owned_signal_refresh(
        contacts,
        spawner.clone(),
        runtime.clone(),
        Arc::clone(app_core),
        "contacts_refresh_hook",
        Arc::new(|app_core| {
            Box::pin(async move { refresh_contacts_and_readiness(&app_core).await })
        }),
        (cancel_rx.clone(), health.clone()),
    )
    .await?;
    spawn_owned_signal_refresh(
        chat,
        spawner.clone(),
        runtime.clone(),
        Arc::clone(app_core),
        "chat_refresh_hook",
        Arc::new(|app_core| {
            Box::pin(async move { refresh_chat_projection_and_readiness(&app_core).await })
        }),
        (cancel_rx.clone(), health.clone()),
    )
    .await?;
    #[cfg(feature = "signals")]
    {
        spawn_owned_signal_refresh(
            chat_readiness,
            spawner.clone(),
            runtime.clone(),
            Arc::clone(app_core),
            "authoritative_chat_readiness_hook",
            Arc::new(|app_core| {
                Box::pin(async move {
                    // Runtime emissions (inbound messages, membership) land
                    // only in CHAT_SIGNAL; the render snapshot follows it here.
                    let mut best_effort = workflow_best_effort();
                    let _ = best_effort
                        .capture(
                            crate::workflows::observed_projection::mirror_chat_signal_into_view(
                                &app_core,
                            ),
                        )
                        .await;
                    let _ = best_effort
                        .capture(refresh_authoritative_channel_and_recipient_readiness_hook(
                            &app_core,
                        ))
                        .await;
                    best_effort.finish()
                })
            }),
            (cancel_rx.clone(), health.clone()),
        )
        .await?;
        spawn_owned_signal_refresh(
            homes_readiness,
            spawner.clone(),
            runtime.clone(),
            Arc::clone(app_core),
            "authoritative_homes_readiness_hook",
            Arc::new(|app_core| {
                Box::pin(async move {
                    // A home joined live (e.g. an accepted home invitation)
                    // must reach the neighborhood projection without a restart.
                    let mut best_effort = workflow_best_effort();
                    let _ = best_effort
                        .capture(
                            crate::workflows::observed_projection::mirror_homes_signal_into_view(
                                &app_core,
                            ),
                        )
                        .await;
                    let _ = best_effort
                        .capture(refresh_authoritative_channel_and_recipient_readiness_hook(
                            &app_core,
                        ))
                        .await;
                    best_effort.finish()
                })
            }),
            (cancel_rx.clone(), health.clone()),
        )
        .await?;
        spawn_owned_signal_refresh(
            peers_readiness,
            spawner.clone(),
            runtime.clone(),
            Arc::clone(app_core),
            "authoritative_transport_peers_readiness_hook",
            Arc::new(|app_core| {
                Box::pin(async move {
                    refresh_authoritative_channel_and_recipient_readiness_hook(&app_core).await
                })
            }),
            (cancel_rx.clone(), health.clone()),
        )
        .await?;
        spawn_owned_signal_refresh(
            invitation_readiness,
            spawner.clone(),
            runtime.clone(),
            Arc::clone(app_core),
            "authoritative_invitations_readiness_hook",
            Arc::new(|app_core| {
                Box::pin(async move {
                    refresh_authoritative_invitation_and_channel_readiness_hook(&app_core).await
                })
            }),
            (cancel_rx.clone(), health.clone()),
        )
        .await?;
        spawn_owned_signal_refresh(
            recovery_projection,
            spawner,
            runtime,
            Arc::clone(app_core),
            "recovery_projection_hook",
            Arc::new(|app_core| {
                Box::pin(async move {
                    crate::workflows::observed_projection::mirror_recovery_signal_into_view(
                        &app_core,
                    )
                    .await
                })
            }),
            (cancel_rx, health.clone()),
        )
        .await?;

        let mut best_effort = workflow_best_effort();
        let _ = best_effort
            .capture(refresh_authoritative_contact_link_readiness_hook(app_core))
            .await;
        let _ = best_effort
            .capture(refresh_authoritative_invitation_and_channel_readiness_hook(
                app_core,
            ))
            .await;
        let _ = best_effort
            .capture(
                crate::workflows::observed_projection::mirror_recovery_signal_into_view(app_core),
            )
            .await;
        let _ = best_effort
            .capture(crate::workflows::observed_projection::mirror_chat_signal_into_view(app_core))
            .await;
        best_effort
            .finish()
            .map_err(|source| HookInstallError::InitialRefresh { source })?;
    }

    spawn_owned_enrollment_completion_refresh(
        enrollment_spawner,
        enrollment_runtime,
        Arc::clone(app_core),
        enrollment_cancel,
        health.clone(),
    )
    .await?;

    if let Some(source) = group.failure().await {
        return Err(HookInstallError::RequiredFailed { source });
    }
    Ok(group)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::AppConfig;
    use crate::runtime_bridge::OfflineRuntimeBridge;
    use aura_core::{AuthorityId, OwnedShutdownToken};
    use aura_effects::reactive::CountingTestTaskSpawner;
    use std::sync::atomic::AtomicUsize;
    use tokio::sync::Notify;

    #[tokio::test]
    async fn offline_completion_refresh_parks_until_hook_cancellation() {
        let runtime: Arc<dyn RuntimeBridge> = Arc::new(OfflineRuntimeBridge::new(
            aura_core::AuthorityId::new_from_entropy([79; 32]),
        ));
        let (cancel_tx, cancel_rx) = oneshot::channel();
        let cancel = cancel_rx.map(|_| ()).boxed().shared();
        let mut wait = Box::pin(await_enrollment_refresh_interval(&runtime, cancel));
        assert!(wait.as_mut().now_or_never().is_none());
        cancel_tx.send(()).unwrap();
        assert!(!wait
            .await
            .expect("cancellation must park without a timer failure"));
    }

    #[tokio::test]
    async fn enrollment_completion_replays_after_app_hook_reattachment() {
        use crate::runtime_bridge::{CeremonyFailureReason, CeremonyTerminalOutcome};
        use crate::ui_contract::{
            AuthoritativeSemanticFact, OperationId, SemanticFailureCode, SemanticOperationPhase,
        };

        let runtime =
            crate::testing::running_offline_runtime(AuthorityId::new_from_entropy([80; 32]));
        runtime.set_pending_invitations(Vec::new());
        let completed = aura_core::CeremonyId::new("hook-replay-completed");
        let rejected = aura_core::CeremonyId::new("hook-replay-rejected");
        runtime.set_enrollment_outcome(completed.clone(), None);
        runtime.set_enrollment_outcome(rejected.clone(), None);

        let first =
            crate::testing::test_app_core_with_runtime(AppConfig::default(), runtime.clone());
        AppCore::init_signals_with_hooks(&first).await.unwrap();
        let first_facts = first.read().await.authoritative_semantic_facts().clone();
        assert!(first_facts.iter().all(|fact| {
            !matches!(fact, AuthoritativeSemanticFact::OperationStatus { status, .. }
                if matches!(status.phase, SemanticOperationPhase::Succeeded | SemanticOperationPhase::Failed))
        }));
        assert!(AppCore::detach_runtime(&first).await);

        runtime.set_enrollment_outcome(completed.clone(), Some(CeremonyTerminalOutcome::Committed));
        runtime.set_enrollment_outcome(
            rejected.clone(),
            Some(CeremonyTerminalOutcome::Failed(
                CeremonyFailureReason::Rejected,
            )),
        );
        let reattached =
            crate::testing::test_app_core_with_runtime(AppConfig::default(), runtime.clone());
        AppCore::init_signals_with_hooks(&reattached).await.unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            loop {
                let facts = reattached.read().await.authoritative_semantic_facts().clone();
                let committed = facts.iter().any(|fact| {
                    matches!(fact,
                        AuthoritativeSemanticFact::OperationStatus { operation_id, status, .. }
                        if operation_id == &OperationId::device_enrollment_completion_for(&completed)
                            && status.phase == SemanticOperationPhase::Succeeded)
                });
                let refused = facts.iter().any(|fact| {
                    matches!(fact,
                        AuthoritativeSemanticFact::OperationStatus { operation_id, status, .. }
                        if operation_id == &OperationId::device_enrollment_completion_for(&rejected)
                            && status.phase == SemanticOperationPhase::Failed
                            && status.error.as_ref().is_some_and(|error|
                                error.code == SemanticFailureCode::CeremonyRejected))
                });
                if committed && refused {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("reattached app hook must replay each runtime result");

        let facts = reattached
            .read()
            .await
            .authoritative_semantic_facts()
            .clone();
        for (ceremony_id, expected_phase) in [
            (&completed, SemanticOperationPhase::Succeeded),
            (&rejected, SemanticOperationPhase::Failed),
        ] {
            let terminal_count = facts
                .iter()
                .filter(|fact| {
                    matches!(fact,
                    AuthoritativeSemanticFact::OperationStatus { operation_id, status, .. }
                    if operation_id == &OperationId::device_enrollment_completion_for(ceremony_id)
                        && status.phase == expected_phase)
                })
                .count();
            assert_eq!(terminal_count, 1, "one terminal fact per ceremony");
        }
        assert!(first.read().await.authoritative_semantic_facts().iter().all(|fact| {
            !matches!(fact, AuthoritativeSemanticFact::OperationStatus { status, .. }
                if matches!(status.phase, SemanticOperationPhase::Succeeded | SemanticOperationPhase::Failed))
        }));
        AppCore::detach_runtime(&reattached).await;
    }

    #[cfg(feature = "signals")]
    #[tokio::test]
    async fn replayed_chat_and_recovery_before_hook_attachment_are_mirrored_on_install() {
        use crate::signal_defs::{CHAT_SIGNAL, RECOVERY_SIGNAL};
        use crate::views::recovery::{Guardian, GuardianStatus, RecoveryState};
        use crate::views::ChatState;

        let runtime =
            crate::testing::running_offline_runtime(AuthorityId::new_from_entropy([75; 32]));
        runtime.set_pending_invitations(Vec::new());
        let app_core = crate::testing::test_app_core_with_runtime(AppConfig::default(), runtime);
        let guardian_id = AuthorityId::new_from_entropy([76; 32]);
        let reactive = { app_core.read().await.reactive().clone() };
        crate::signal_defs::register_app_signals(&reactive)
            .await
            .unwrap();
        let mut replayed_chat = ChatState::default();
        replayed_chat.total_unread = 3;
        reactive
            .graph()
            .emit(CHAT_SIGNAL.id(), replayed_chat)
            .await
            .unwrap();
        reactive
            .graph()
            .emit(
                RECOVERY_SIGNAL.id(),
                RecoveryState::from_parts(
                    [Guardian {
                        id: guardian_id,
                        name: "replayed guardian".to_string(),
                        status: GuardianStatus::Active,
                        added_at: 1,
                        last_seen: None,
                    }],
                    1,
                    None,
                    Vec::new(),
                    Vec::new(),
                ),
            )
            .await
            .unwrap();

        AppCore::init_signals_with_hooks(&app_core).await.unwrap();
        let snapshot = app_core.read().await.snapshot();
        // Initial runtime refresh may supersede the replayed fixture; the
        // app must mirror whichever graph revision is current after attach.
        assert_eq!(
            snapshot.chat.total_unread,
            reactive
                .read_snapshot(&*CHAT_SIGNAL)
                .await
                .unwrap()
                .value
                .total_unread
        );
        assert_eq!(
            snapshot.projection_source_revisions.chat,
            Some(
                reactive
                    .read_snapshot(&*CHAT_SIGNAL)
                    .await
                    .unwrap()
                    .revision
            )
        );
        assert_eq!(
            snapshot
                .recovery
                .guardian(&guardian_id)
                .map(|guardian| guardian.name.as_str()),
            Some("replayed guardian")
        );
        assert_eq!(
            snapshot.projection_source_revisions.recovery,
            Some(
                reactive
                    .read_snapshot(&*RECOVERY_SIGNAL)
                    .await
                    .unwrap()
                    .revision
            )
        );
    }

    #[tokio::test]
    async fn blocked_refresh_replays_latest_snapshot_after_burst_and_lag() {
        const FINAL_REVISION: usize = 1024;
        let counting_spawner = Arc::new(CountingTestTaskSpawner::default());
        let mut runtime = OfflineRuntimeBridge::new(AuthorityId::new_from_entropy([71; 32]));
        runtime.use_test_task_spawner(OwnedTaskSpawner::new(
            counting_spawner.clone(),
            OwnedShutdownToken::detached(),
        ));
        let runtime = Arc::new(runtime);
        let app_core =
            crate::testing::test_app_core_with_runtime(AppConfig::default(), runtime.clone());
        let reactive = { app_core.read().await.reactive().clone() };
        let signal = Signal::<u32>::new("test:owned-refresh-burst");
        reactive
            .graph()
            .ensure_registered(signal.id().clone(), 0u32)
            .await
            .unwrap();
        let stream = reactive.subscribe_attached(&signal).await.unwrap();
        let latest_source = Arc::new(AtomicUsize::new(0));
        let latest_seen = Arc::new(AtomicUsize::new(0));
        let passes = Arc::new(AtomicUsize::new(0));
        let active = Arc::new(AtomicUsize::new(0));
        let max_active = Arc::new(AtomicUsize::new(0));
        let first_entered = Arc::new(Notify::new());
        let release_first = Arc::new(Notify::new());
        let final_seen = Arc::new(Notify::new());

        let refresh: RefreshHook = {
            let latest_source = latest_source.clone();
            let latest_seen = latest_seen.clone();
            let passes = passes.clone();
            let active = active.clone();
            let max_active = max_active.clone();
            let first_entered = first_entered.clone();
            let release_first = release_first.clone();
            let final_seen = final_seen.clone();
            Arc::new(move |_| {
                let latest_source = latest_source.clone();
                let latest_seen = latest_seen.clone();
                let passes = passes.clone();
                let active = active.clone();
                let max_active = max_active.clone();
                let first_entered = first_entered.clone();
                let release_first = release_first.clone();
                let final_seen = final_seen.clone();
                Box::pin(async move {
                    let pass = passes.fetch_add(1, Ordering::SeqCst) + 1;
                    let current_active = active.fetch_add(1, Ordering::SeqCst) + 1;
                    max_active.fetch_max(current_active, Ordering::SeqCst);
                    let snapshot = latest_source.load(Ordering::SeqCst);
                    if pass == 1 {
                        first_entered.notify_one();
                        release_first.notified().await;
                    }
                    latest_seen.store(snapshot, Ordering::SeqCst);
                    active.fetch_sub(1, Ordering::SeqCst);
                    if snapshot == FINAL_REVISION {
                        final_seen.notify_one();
                    }
                    Ok(())
                })
            })
        };
        let (cancel_tx, cancel_rx) = oneshot::channel();
        let cancel = cancel_rx.map(|_| ()).boxed().shared();
        let runtime_bridge: Arc<dyn RuntimeBridge> = runtime.clone();
        spawn_owned_signal_refresh(
            stream,
            runtime.task_spawner(),
            runtime_bridge,
            app_core,
            "test_owned_refresh",
            refresh,
            (cancel, HookHealth::new().0),
        )
        .await
        .unwrap();
        assert_eq!(counting_spawner.spawned_count(), 1);

        latest_source.store(1, Ordering::SeqCst);
        reactive.graph().emit(signal.id(), 1u32).await.unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(2), first_entered.notified())
            .await
            .unwrap();
        for value in 2..=FINAL_REVISION {
            latest_source.store(value, Ordering::SeqCst);
            reactive
                .graph()
                .emit(signal.id(), value as u32)
                .await
                .unwrap();
        }
        assert_eq!(passes.load(Ordering::SeqCst), 1);
        assert_eq!(max_active.load(Ordering::SeqCst), 1);
        release_first.notify_one();
        tokio::time::timeout(std::time::Duration::from_secs(2), final_seen.notified())
            .await
            .unwrap();
        assert_eq!(latest_seen.load(Ordering::SeqCst), FINAL_REVISION);
        assert!(passes.load(Ordering::SeqCst) >= 2);
        assert_eq!(max_active.load(Ordering::SeqCst), 1);
        assert_eq!(counting_spawner.spawned_count(), 1);

        cancel_tx.send(()).unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            while reactive.graph().subscriber_count(signal.id()).await != 0 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
    }

    #[cfg(feature = "signals")]
    #[tokio::test]
    async fn installed_hooks_converge_contacts_chat_invitation_and_readiness() {
        use crate::signal_defs::{
            SyncStatus, CHAT_SIGNAL, CONTACTS_SIGNAL, INVITATIONS_SIGNAL, RECOVERY_SIGNAL,
            SYNC_STATUS_SIGNAL,
        };
        use crate::ui_contract::AuthoritativeSemanticFact;
        use crate::views::contacts::{
            Contact, ContactRelationshipState, ContactsState, ReadReceiptPolicy,
        };
        use crate::views::invitations::{
            Invitation, InvitationDirection, InvitationStatus, InvitationType, InvitationsState,
        };
        use crate::views::recovery::{Guardian, GuardianStatus, RecoveryState};
        use crate::views::ChatState;

        let runtime =
            crate::testing::running_offline_runtime(AuthorityId::new_from_entropy([72; 32]));
        runtime.set_pending_invitations(Vec::new());
        let app_core = crate::testing::test_app_core_with_runtime(AppConfig::default(), runtime);
        AppCore::init_signals_with_hooks(&app_core).await.unwrap();
        let reactive = { app_core.read().await.reactive().clone() };
        let contact_id = AuthorityId::new_from_entropy([73; 32]);

        for revision in 1..=8 {
            let contact = Contact {
                id: contact_id,
                nickname: format!("contact-{revision}"),
                nickname_suggestion: None,
                is_guardian: false,
                is_member: false,
                last_interaction: None,
                is_online: false,
                read_receipt_policy: ReadReceiptPolicy::default(),
                relationship_state: ContactRelationshipState::default(),
                invitation_code: None,
            };
            reactive
                .graph()
                .emit(
                    CONTACTS_SIGNAL.id(),
                    ContactsState::from_contacts([contact]),
                )
                .await
                .unwrap();
            let mut revised_chat = ChatState::default();
            revised_chat.total_unread = revision;
            reactive
                .graph()
                .emit(CHAT_SIGNAL.id(), revised_chat)
                .await
                .unwrap();
            reactive
                .graph()
                .emit(
                    SYNC_STATUS_SIGNAL.id(),
                    SyncStatus::Syncing {
                        progress: revision as u8,
                    },
                )
                .await
                .unwrap();
            let invitation = Invitation {
                id: format!("home-invitation-{revision}"),
                invitation_type: InvitationType::Home,
                status: InvitationStatus::Pending,
                direction: InvitationDirection::Received,
                from_id: contact_id,
                from_name: "Contact".to_owned(),
                to_id: None,
                to_name: None,
                created_at: revision as u64,
                expires_at: None,
                message: None,
                home_id: Some(aura_core::ChannelId::from_bytes([74; 32])),
                home_name: Some("Shared Home".to_owned()),
            };
            reactive
                .graph()
                .emit(
                    INVITATIONS_SIGNAL.id(),
                    InvitationsState::from_parts(vec![invitation], Vec::new(), Vec::new()),
                )
                .await
                .unwrap();
            reactive
                .graph()
                .emit(
                    RECOVERY_SIGNAL.id(),
                    RecoveryState::from_parts(
                        [Guardian {
                            id: contact_id,
                            name: format!("guardian-{revision}"),
                            status: GuardianStatus::Active,
                            added_at: revision as u64,
                            last_seen: None,
                        }],
                        1,
                        None,
                        Vec::new(),
                        Vec::new(),
                    ),
                )
                .await
                .unwrap();
        }

        let convergence = tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                let core = app_core.read().await;
                let snapshot = core.snapshot();
                let converged = snapshot
                    .contacts
                    .contact(&contact_id)
                    .is_some_and(|contact| contact.nickname == "contact-8")
                    && snapshot.chat.total_unread == 8
                    && snapshot
                        .recovery
                        .guardian(&contact_id)
                        .is_some_and(|guardian| guardian.name == "guardian-8")
                    && snapshot.projection_source_revisions.recovery.is_some()
                    && snapshot
                        .invitations
                        .all_pending()
                        .iter()
                        .any(|invitation| invitation.id == "home-invitation-8")
                    && core
                        .authoritative_semantic_facts()
                        .contains(&AuthoritativeSemanticFact::PendingHomeInvitationReady);
                drop(core);
                if converged {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await;
        if convergence.is_err() {
            let core = app_core.read().await;
            let snapshot = core.snapshot();
            let chat_signal = core
                .reactive()
                .graph()
                .read::<ChatState>(CHAT_SIGNAL.id())
                .await
                .ok();
            panic!(
                "installed hooks did not converge: contact={:?}, unread={}, chat_signal_unread={:?}, recovery={:?}, invitations={:?}, facts={:?}",
                snapshot.contacts.contact(&contact_id).map(|contact| &contact.nickname),
                snapshot.chat.total_unread,
                chat_signal.map(|chat| chat.total_unread),
                snapshot.recovery.guardian(&contact_id).map(|guardian| &guardian.name),
                snapshot.invitations.all_pending().iter().map(|invitation| &invitation.id).collect::<Vec<_>>(),
                core.authoritative_semantic_facts(),
            );
        }
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod required_hook_health_tests {
    use super::*;
    use aura_core::OwnedShutdownToken;
    use aura_effects::reactive::CountingTestTaskSpawner;
    use std::error::Error;
    use std::sync::atomic::AtomicUsize;

    #[derive(Debug, thiserror::Error)]
    #[error("required projection storage unavailable")]
    struct ProjectionStorageFault;

    #[tokio::test]
    async fn actual_signal_owner_retains_update_fault_and_keeps_attachment() {
        let spawner = Arc::new(CountingTestTaskSpawner::default());
        let mut runtime = crate::runtime_bridge::OfflineRuntimeBridge::new(
            aura_core::AuthorityId::new_from_entropy([173; 32]),
        );
        runtime.use_test_task_spawner(OwnedTaskSpawner::new(
            spawner.clone(),
            OwnedShutdownToken::detached(),
        ));
        let runtime = Arc::new(runtime);
        let app = crate::testing::test_app_core_with_runtime(
            crate::core::AppConfig::default(),
            runtime.clone(),
        );
        let reactive = app.read().await.reactive().clone();
        let signal = Signal::<u32>::new("test:required-hook-fault");
        reactive
            .graph()
            .ensure_registered(signal.id().clone(), 0u32)
            .await
            .expect("register real signal");
        let stream = reactive
            .subscribe_attached(&signal)
            .await
            .expect("attach real signal");
        let (health, cancellation) = HookHealth::new();
        let group = HookGroup {
            health: health.clone(),
            cancellation: cancellation.clone(),
            shutdown: OwnedShutdownToken::detached(),
        };
        let calls = Arc::new(AtomicUsize::new(0));
        let refresh_calls = calls.clone();
        let refresh: RefreshHook = Arc::new(move |_| {
            let call = refresh_calls.fetch_add(1, Ordering::SeqCst);
            Box::pin(async move {
                if call == 0 {
                    Err(AuraError::Storage {
                        message: "required projection read failed".into(),
                        source: Some(Arc::new(ProjectionStorageFault)),
                    })
                } else {
                    Ok(())
                }
            })
        });
        spawn_owned_signal_refresh(
            stream,
            runtime.task_spawner(),
            runtime,
            app,
            "actual-required-hook",
            refresh,
            (cancellation.clone(), health),
        )
        .await
        .expect("admit actual owned listener");
        reactive
            .graph()
            .emit(signal.id(), 1u32)
            .await
            .expect("emit real signal");
        let failure = tokio::time::timeout(std::time::Duration::from_secs(2), async {
            loop {
                if let Some(failure) = group.update_failures().await.into_iter().next() {
                    break failure;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("the failed update must be retained");
        assert_eq!(failure.stage(), HookFailureStage::Refresh);
        assert_eq!(failure.name(), "actual-required-hook");
        assert!(failure
            .native_error()
            .source()
            .expect("provider source")
            .is::<ProjectionStorageFault>());
        assert!(group.is_active(), "one failed update keeps the attachment");
        assert!(group.failure().await.is_none());
        assert!(spawner.failure().await.is_none());

        reactive
            .graph()
            .emit(signal.id(), 2u32)
            .await
            .expect("emit real signal");
        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            while calls.load(Ordering::SeqCst) < 2 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("the listener must refresh the next update");
        assert!(group.is_active());
        group
            .publish_ready(|_| {})
            .await
            .expect("an active attachment still publishes Ready");
    }
}
