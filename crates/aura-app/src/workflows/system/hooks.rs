//! Background refresh-hook installation for system-owned derived state.

use super::refresh::{emit_chat_snapshot_signal, refresh_connection_status_from_contacts};
use crate::runtime_bridge::RuntimeBridge;
#[cfg(feature = "signals")]
use crate::signal_defs::{CHAT_SIGNAL, HOMES_SIGNAL, INVITATIONS_SIGNAL, TRANSPORT_PEERS_SIGNAL};
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
    #[cfg(test)]
    #[error("injected hook attachment failure at step {step}")]
    Injected { step: usize },
}

fn log_refresh_hook_error(refresh_name: &'static str, error: &AuraError) {
    #[cfg(feature = "instrumented")]
    tracing::warn!(refresh_name, error = %error, "system refresh hook pass failed");

    #[cfg(not(feature = "instrumented"))]
    let _ = (refresh_name, error);
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

async fn spawn_coalesced_signal_refresh<T>(
    mut stream: SignalStream<T>,
    spawner: OwnedTaskSpawner,
    runtime: Arc<dyn RuntimeBridge>,
    app_core: Arc<RwLock<AppCore>>,
    refresh_name: &'static str,
    refresh: RefreshHook,
    cancel: HookCancellation,
) -> Result<(), HookInstallError>
where
    T: Clone + Send + Sync + 'static,
{
    let refresh_in_flight = Arc::new(AtomicBool::new(false));
    let refresh_pending = Arc::new(AtomicBool::new(false));
    let refresh_spawner = spawner.clone();
    let (started_tx, started_rx) = oneshot::channel();

    spawn_cancellable_runtime_refresh_task(&spawner, async move {
        let _ = started_tx.send(());
        loop {
            let received = futures::select! {
                _ = cancel.clone().fuse() => break,
                received = stream.recv().fuse() => received,
            };
            let Ok(_) = received else {
                break;
            };

            if refresh_in_flight.swap(true, Ordering::SeqCst) {
                refresh_pending.store(true, Ordering::SeqCst);
                continue;
            }

            let refresh_app_core = app_core.clone();
            let refresh_in_flight = refresh_in_flight.clone();
            let refresh_pending = refresh_pending.clone();
            let refresh = refresh.clone();
            let refresh_cancel = cancel.clone();
            spawn_runtime_refresh_task(&refresh_spawner, async move {
                loop {
                    let outcome = futures::select! {
                        _ = refresh_cancel.clone().fuse() => break,
                        outcome = refresh(refresh_app_core.clone()).fuse() => outcome,
                    };
                    if let Err(error) = outcome {
                        log_refresh_hook_error(refresh_name, &error);
                    }

                    if refresh_pending.swap(false, Ordering::SeqCst) {
                        continue;
                    }

                    refresh_in_flight.store(false, Ordering::SeqCst);
                    break;
                }
            });
        }
    });
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

#[cfg(not(target_arch = "wasm32"))]
fn spawn_runtime_refresh_task<F>(spawner: &OwnedTaskSpawner, fut: F)
where
    F: Future<Output = ()> + Send + 'static,
{
    spawner.spawn(Box::pin(fut));
}

#[cfg(target_arch = "wasm32")]
fn spawn_runtime_refresh_task<F>(spawner: &OwnedTaskSpawner, fut: F)
where
    F: Future<Output = ()> + 'static,
{
    spawner.spawn_local(Box::pin(fut));
}

#[cfg(not(target_arch = "wasm32"))]
fn spawn_cancellable_runtime_refresh_task<F>(spawner: &OwnedTaskSpawner, fut: F)
where
    F: Future<Output = ()> + Send + 'static,
{
    spawner.spawn_cancellable(Box::pin(fut));
}

#[cfg(target_arch = "wasm32")]
fn spawn_cancellable_runtime_refresh_task<F>(spawner: &OwnedTaskSpawner, fut: F)
where
    F: Future<Output = ()> + 'static,
{
    spawner.spawn_local_cancellable(Box::pin(fut));
}

/// Owns all refresh subscriptions for one runtime generation.
pub(crate) struct HookGroup {
    cancel: Option<oneshot::Sender<()>>,
    cancelled: Arc<AtomicBool>,
    #[cfg(test)]
    cancellation: HookCancellation,
    shutdown: aura_core::OwnedShutdownToken,
}

impl HookGroup {
    pub(crate) fn is_active(&self) -> bool {
        !self.cancelled.load(Ordering::SeqCst) && !self.shutdown.is_cancelled()
    }

    #[cfg(test)]
    pub(crate) fn cancellation_receiver(&self) -> HookCancellation {
        self.cancellation.clone()
    }
}

impl Drop for HookGroup {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::SeqCst);
        if let Some(cancel) = self.cancel.take() {
            let _ = cancel.send(());
        }
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

    let (cancel, cancel_rx) = oneshot::channel();
    let cancel_rx = cancel_rx.map(|_| ()).boxed().shared();
    let group = HookGroup {
        cancel: Some(cancel),
        cancelled: Arc::new(AtomicBool::new(false)),
        #[cfg(test)]
        cancellation: cancel_rx.clone(),
        shutdown: spawner.shutdown_token().clone(),
    };
    spawn_coalesced_signal_refresh(
        contacts,
        spawner.clone(),
        runtime.clone(),
        Arc::clone(app_core),
        "contacts_refresh_hook",
        Arc::new(|app_core| {
            Box::pin(async move { refresh_contacts_and_readiness(&app_core).await })
        }),
        cancel_rx.clone(),
    )
    .await?;
    spawn_coalesced_signal_refresh(
        chat,
        spawner.clone(),
        runtime.clone(),
        Arc::clone(app_core),
        "chat_refresh_hook",
        Arc::new(|app_core| {
            Box::pin(async move { refresh_chat_projection_and_readiness(&app_core).await })
        }),
        cancel_rx.clone(),
    )
    .await?;
    #[cfg(feature = "signals")]
    {
        spawn_coalesced_signal_refresh(
            chat_readiness,
            spawner.clone(),
            runtime.clone(),
            Arc::clone(app_core),
            "authoritative_chat_readiness_hook",
            Arc::new(|app_core| {
                Box::pin(async move {
                    refresh_authoritative_channel_and_recipient_readiness_hook(&app_core).await
                })
            }),
            cancel_rx.clone(),
        )
        .await?;
        spawn_coalesced_signal_refresh(
            homes_readiness,
            spawner.clone(),
            runtime.clone(),
            Arc::clone(app_core),
            "authoritative_homes_readiness_hook",
            Arc::new(|app_core| {
                Box::pin(async move {
                    refresh_authoritative_channel_and_recipient_readiness_hook(&app_core).await
                })
            }),
            cancel_rx.clone(),
        )
        .await?;
        spawn_coalesced_signal_refresh(
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
            cancel_rx.clone(),
        )
        .await?;
        spawn_coalesced_signal_refresh(
            invitation_readiness,
            spawner,
            runtime,
            Arc::clone(app_core),
            "authoritative_invitations_readiness_hook",
            Arc::new(|app_core| {
                Box::pin(async move {
                    refresh_authoritative_invitation_and_channel_readiness_hook(&app_core).await
                })
            }),
            cancel_rx,
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
        best_effort
            .finish()
            .map_err(|source| HookInstallError::InitialRefresh { source })?;
    }

    Ok(group)
}
