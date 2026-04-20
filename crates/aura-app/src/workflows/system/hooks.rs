//! Background refresh-hook installation for system-owned derived state.

use super::refresh::{emit_chat_snapshot_signal, refresh_connection_status_from_contacts};
#[cfg(feature = "signals")]
use crate::signal_defs::{CHAT_SIGNAL, HOMES_SIGNAL, INVITATIONS_SIGNAL, TRANSPORT_PEERS_SIGNAL};
use crate::signal_defs::{CONTACTS_SIGNAL, SYNC_STATUS_SIGNAL};
use crate::workflows::runtime::workflow_best_effort;
use crate::{AppCore, ReactiveHandler};
use async_lock::RwLock;
use aura_core::effects::reactive::{ReactiveEffects, Signal};
use aura_core::{AuraError, OwnedTaskSpawner};
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

#[cfg(not(target_arch = "wasm32"))]
type BoxRefreshFuture = Pin<Box<dyn Future<Output = Result<(), AuraError>> + Send + 'static>>;
#[cfg(target_arch = "wasm32")]
type BoxRefreshFuture = Pin<Box<dyn Future<Output = Result<(), AuraError>> + 'static>>;
type RefreshHook = Arc<dyn Fn(Arc<RwLock<AppCore>>) -> BoxRefreshFuture + Send + Sync + 'static>;

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
    }
    best_effort.finish()
}

#[cfg(feature = "signals")]
async fn refresh_authoritative_contact_link_readiness_hook(
    app_core: &Arc<RwLock<AppCore>>,
) -> Result<(), AuraError> {
    crate::workflows::invitation::refresh_authoritative_contact_link_readiness(app_core).await
}

#[cfg(feature = "signals")]
async fn refresh_authoritative_invitation_and_channel_readiness_hook(
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
    reactive: ReactiveHandler,
    signal: &'static Signal<T>,
    spawner: OwnedTaskSpawner,
    app_core: Arc<RwLock<AppCore>>,
    refresh_name: &'static str,
    refresh: RefreshHook,
) -> Result<(), AuraError>
where
    T: Clone + Send + Sync + 'static,
{
    let mut stream = reactive
        .subscribe(signal)
        .map_err(|error| AuraError::internal(error.to_string()))?;
    let refresh_in_flight = Arc::new(AtomicBool::new(false));
    let refresh_pending = Arc::new(AtomicBool::new(false));
    let refresh_spawner = spawner.clone();

    spawn_cancellable_runtime_refresh_task(&spawner, async move {
        loop {
            let Ok(_) = stream.recv().await else {
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
            spawn_runtime_refresh_task(&refresh_spawner, async move {
                loop {
                    if let Err(error) = refresh(refresh_app_core.clone()).await {
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

    Ok(())
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

pub async fn install_contacts_refresh_hook(
    app_core: &Arc<RwLock<AppCore>>,
) -> Result<(), AuraError> {
    let (reactive, spawner, should_install) = {
        let core = app_core.read().await;
        let already_installed = core.contacts_refresh_hook_installed();
        let reactive = core.reactive().clone();
        let spawner = core.runtime().map(|runtime| runtime.task_spawner());
        (reactive, spawner, !already_installed)
    };

    if !should_install {
        return Ok(());
    }

    let Some(spawner) = spawner else {
        #[cfg(feature = "instrumented")]
        tracing::warn!("contacts refresh hook not installed: no task spawner available");
        return Err(AuraError::from(
            crate::workflows::error::WorkflowError::Precondition(
                "contacts refresh hook requires a runtime task spawner",
            ),
        ));
    };

    {
        let mut core = app_core.write().await;
        if !core.mark_contacts_refresh_hook_installed() {
            return Ok(());
        }
    }

    spawn_coalesced_signal_refresh(
        reactive,
        &*CONTACTS_SIGNAL,
        spawner,
        Arc::clone(app_core),
        "contacts_refresh_hook",
        Arc::new(|app_core| {
            Box::pin(async move { refresh_connection_status_from_contacts(&app_core).await })
        }),
    )
    .await
}

pub async fn install_chat_refresh_hook(app_core: &Arc<RwLock<AppCore>>) -> Result<(), AuraError> {
    let (reactive, spawner, should_install) = {
        let core = app_core.read().await;
        let already_installed = core.chat_refresh_hook_installed();
        let reactive = core.reactive().clone();
        let spawner = core.runtime().map(|runtime| runtime.task_spawner());
        (reactive, spawner, !already_installed)
    };

    if !should_install {
        return Ok(());
    }

    let Some(spawner) = spawner else {
        #[cfg(feature = "instrumented")]
        tracing::warn!("chat refresh hook not installed: no task spawner available");
        return Err(AuraError::from(
            crate::workflows::error::WorkflowError::Precondition(
                "chat refresh hook requires a runtime task spawner",
            ),
        ));
    };

    {
        let mut core = app_core.write().await;
        if !core.mark_chat_refresh_hook_installed() {
            return Ok(());
        }
    }

    spawn_coalesced_signal_refresh(
        reactive,
        &*SYNC_STATUS_SIGNAL,
        spawner,
        Arc::clone(app_core),
        "chat_refresh_hook",
        Arc::new(|app_core| {
            Box::pin(async move { refresh_chat_projection_and_readiness(&app_core).await })
        }),
    )
    .await
}

#[cfg(feature = "signals")]
pub async fn install_authoritative_readiness_hook(
    app_core: &Arc<RwLock<AppCore>>,
) -> Result<(), AuraError> {
    let (reactive, spawner, should_install) = {
        let core = app_core.read().await;
        let already_installed = core.authoritative_readiness_hook_installed();
        let reactive = core.reactive().clone();
        let spawner = core.runtime().map(|runtime| runtime.task_spawner());
        (reactive, spawner, !already_installed)
    };

    if !should_install {
        return Ok(());
    }

    let Some(spawner) = spawner else {
        #[cfg(feature = "instrumented")]
        tracing::warn!("authoritative readiness hook not installed: no task spawner available");
        return Err(AuraError::from(
            crate::workflows::error::WorkflowError::Precondition(
                "authoritative readiness hook requires a runtime task spawner",
            ),
        ));
    };

    {
        let mut core = app_core.write().await;
        if !core.mark_authoritative_readiness_hook_installed() {
            return Ok(());
        }
    }

    spawn_coalesced_signal_refresh(
        reactive.clone(),
        &*CONTACTS_SIGNAL,
        spawner.clone(),
        Arc::clone(app_core),
        "authoritative_contact_link_readiness_hook",
        Arc::new(|app_core| {
            Box::pin(
                async move { refresh_authoritative_contact_link_readiness_hook(&app_core).await },
            )
        }),
    )
    .await?;
    spawn_coalesced_signal_refresh(
        reactive.clone(),
        &*CHAT_SIGNAL,
        spawner.clone(),
        Arc::clone(app_core),
        "authoritative_chat_readiness_hook",
        Arc::new(|app_core| {
            Box::pin(async move {
                refresh_authoritative_channel_and_recipient_readiness_hook(&app_core).await
            })
        }),
    )
    .await?;
    spawn_coalesced_signal_refresh(
        reactive.clone(),
        &*HOMES_SIGNAL,
        spawner.clone(),
        Arc::clone(app_core),
        "authoritative_homes_readiness_hook",
        Arc::new(|app_core| {
            Box::pin(async move {
                refresh_authoritative_channel_and_recipient_readiness_hook(&app_core).await
            })
        }),
    )
    .await?;
    spawn_coalesced_signal_refresh(
        reactive.clone(),
        &*TRANSPORT_PEERS_SIGNAL,
        spawner.clone(),
        Arc::clone(app_core),
        "authoritative_transport_peers_readiness_hook",
        Arc::new(|app_core| {
            Box::pin(async move {
                refresh_authoritative_channel_and_recipient_readiness_hook(&app_core).await
            })
        }),
    )
    .await?;
    spawn_coalesced_signal_refresh(
        reactive.clone(),
        &*SYNC_STATUS_SIGNAL,
        spawner.clone(),
        Arc::clone(app_core),
        "authoritative_sync_status_readiness_hook",
        Arc::new(|app_core| {
            Box::pin(async move {
                refresh_authoritative_channel_and_recipient_readiness_hook(&app_core).await
            })
        }),
    )
    .await?;
    spawn_coalesced_signal_refresh(
        reactive,
        &*INVITATIONS_SIGNAL,
        spawner,
        Arc::clone(app_core),
        "authoritative_invitations_readiness_hook",
        Arc::new(|app_core| {
            Box::pin(async move {
                refresh_authoritative_invitation_and_channel_readiness_hook(&app_core).await
            })
        }),
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
    best_effort.finish()
}

#[cfg(not(feature = "signals"))]
pub async fn install_authoritative_readiness_hook(
    _app_core: &Arc<RwLock<AppCore>>,
) -> Result<(), AuraError> {
    Ok(())
}
