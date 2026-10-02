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
    cancel: HookCancellation,
) -> Result<(), HookInstallError>
where
    T: Clone + Send + Sync + 'static,
{
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

            // This task is the sole refresh owner. Updates received while the
            // refresh awaits remain in the bounded signal stream; after a lag,
            // recv resumes with a newer snapshot and refresh reads current state.
            let outcome = futures::select! {
                _ = cancel.clone().fuse() => break,
                outcome = refresh(app_core.clone()).fuse() => outcome,
            };
            if let Err(error) = outcome {
                log_refresh_hook_error(refresh_name, &error);
            }
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
    #[cfg(feature = "signals")]
    let recovery_projection = attempt.attach(&reactive, &*RECOVERY_SIGNAL).await?;

    let (cancel, cancel_rx) = oneshot::channel();
    let cancel_rx = cancel_rx.map(|_| ()).boxed().shared();
    let group = HookGroup {
        cancel: Some(cancel),
        cancelled: Arc::new(AtomicBool::new(false)),
        #[cfg(test)]
        cancellation: cancel_rx.clone(),
        shutdown: spawner.shutdown_token().clone(),
    };
    spawn_owned_signal_refresh(
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
    spawn_owned_signal_refresh(
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
        spawn_owned_signal_refresh(
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
        spawn_owned_signal_refresh(
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
            cancel_rx.clone(),
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
            cancel_rx.clone(),
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
        reactive
            .graph()
            .emit(
                CHAT_SIGNAL.id(),
                ChatState {
                    total_unread: 3,
                    ..Default::default()
                },
            )
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
            cancel,
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
            reactive
                .graph()
                .emit(
                    CHAT_SIGNAL.id(),
                    ChatState {
                        total_unread: revision,
                        ..Default::default()
                    },
                )
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
