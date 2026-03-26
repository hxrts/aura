use async_lock::RwLock;
use aura_agent::AuraAgent;
use aura_app::ui::workflows::network as network_workflows;
use aura_app::ui::workflows::runtime as runtime_workflows;
use aura_app::ui::workflows::system as system_workflows;
use aura_app::ui::workflows::time as time_workflows;
use aura_app::AppCore;
use aura_ui::{FrontendUiOperation as WebUiOperation, UiController};
use std::cell::RefCell;
use std::future::Future;
use std::sync::Arc;

use crate::error::{log_web_error, WebUiError};
use crate::task_owner::{new_web_task_owner, WebTaskOwner};

#[derive(Clone)]
struct GenerationMaintenanceOwner {
    owner: WebTaskOwner,
}

thread_local! {
    static ACTIVE_GENERATION_MAINTENANCE: RefCell<Option<GenerationMaintenanceOwner>> = const { RefCell::new(None) };
}

fn replace_generation_maintenance_owner() -> WebTaskOwner {
    let owner = new_web_task_owner();
    ACTIVE_GENERATION_MAINTENANCE.with(|slot| {
        if let Some(active) = slot.borrow_mut().take() {
            active.owner.shutdown();
        }
        slot.borrow_mut().replace(GenerationMaintenanceOwner {
            owner: owner.clone(),
        });
    });
    owner
}

pub(crate) fn cancel_generation_maintenance_loops() {
    ACTIVE_GENERATION_MAINTENANCE.with(|slot| {
        if let Some(active) = slot.borrow_mut().take() {
            active.owner.shutdown();
        }
    });
}

pub(crate) fn spawn_browser_maintenance_loop<F, Fut>(
    owner: &WebTaskOwner,
    controller: Arc<UiController>,
    app_core: Arc<RwLock<AppCore>>,
    interval_ms: u64,
    pause_message: &'static str,
    sleep_operation: WebUiOperation,
    sleep_error_code: &'static str,
    mut tick: F,
) where
    F: FnMut() -> Fut + 'static,
    Fut: Future<Output = ()> + 'static,
{
    owner.spawn_local_cancellable(async move {
        loop {
            if let Err(error) = time_workflows::sleep_ms(&app_core, interval_ms).await {
                log_web_error(
                    "warn",
                    &WebUiError::operation(sleep_operation, sleep_error_code, error.to_string()),
                );
                controller.runtime_error_toast(pause_message);
                break;
            }
            tick().await;
        }
    });
}

pub(crate) fn spawn_background_sync_loop(
    owner: &WebTaskOwner,
    controller: Arc<UiController>,
    app_core: Arc<RwLock<AppCore>>,
) {
    let tick_app_core = app_core.clone();
    spawn_browser_maintenance_loop(
        owner,
        controller,
        app_core,
        1_500,
        "Background sync paused; refresh to resume",
        WebUiOperation::BackgroundSync,
        "WEB_BACKGROUND_SYNC_SLEEP_FAILED",
        move || {
            let tick_app_core = tick_app_core.clone();
            async move {
                let runtime = { tick_app_core.read().await.runtime().cloned() };
                if let Some(runtime) = runtime {
                    if let Err(error) = runtime_workflows::timeout_runtime_call(
                        &runtime,
                        "web_background_sync",
                        "trigger_discovery",
                        std::time::Duration::from_secs(3),
                        || runtime.trigger_discovery(),
                    )
                    .await
                    {
                        log_web_error(
                            "warn",
                            &WebUiError::operation(
                                WebUiOperation::BackgroundSync,
                                "WEB_DISCOVERY_TRIGGER_FAILED",
                                error.to_string(),
                            ),
                        );
                    }
                    if let Err(error) = runtime_workflows::timeout_runtime_call(
                        &runtime,
                        "web_background_sync",
                        "process_ceremony_messages_before_sync",
                        std::time::Duration::from_secs(3),
                        || runtime.process_ceremony_messages(),
                    )
                    .await
                    {
                        log_web_error(
                            "warn",
                            &WebUiError::operation(
                                WebUiOperation::BackgroundSync,
                                "WEB_CEREMONY_MESSAGES_BEFORE_SYNC_FAILED",
                                error.to_string(),
                            ),
                        );
                    }
                    if let Err(error) = runtime_workflows::timeout_runtime_call(
                        &runtime,
                        "web_background_sync",
                        "trigger_sync",
                        std::time::Duration::from_secs(3),
                        || runtime.trigger_sync(),
                    )
                    .await
                    {
                        log_web_error(
                            "warn",
                            &WebUiError::operation(
                                WebUiOperation::BackgroundSync,
                                "WEB_SYNC_TRIGGER_FAILED",
                                error.to_string(),
                            ),
                        );
                    }
                    if let Err(error) = runtime_workflows::timeout_runtime_call(
                        &runtime,
                        "web_background_sync",
                        "process_ceremony_messages_after_sync",
                        std::time::Duration::from_secs(3),
                        || runtime.process_ceremony_messages(),
                    )
                    .await
                    {
                        log_web_error(
                            "warn",
                            &WebUiError::operation(
                                WebUiOperation::BackgroundSync,
                                "WEB_CEREMONY_MESSAGES_AFTER_SYNC_FAILED",
                                error.to_string(),
                            ),
                        );
                    }
                }
                if let Err(error) = system_workflows::refresh_account(&tick_app_core).await {
                    log_web_error(
                        "warn",
                        &WebUiError::operation(
                            WebUiOperation::BackgroundSync,
                            "WEB_REFRESH_ACCOUNT_FAILED",
                            error.to_string(),
                        ),
                    );
                }
                if let Err(error) =
                    network_workflows::refresh_discovered_peers(&tick_app_core).await
                {
                    log_web_error(
                        "warn",
                        &WebUiError::operation(
                            WebUiOperation::BackgroundSync,
                            "WEB_DISCOVERED_PEERS_REFRESH_FAILED",
                            error.to_string(),
                        ),
                    );
                }
            }
        },
    );
}

fn spawn_ceremony_acceptance_loop(
    owner: &WebTaskOwner,
    controller: Arc<UiController>,
    app_core: Arc<RwLock<AppCore>>,
    agent: Arc<AuraAgent>,
) {
    spawn_browser_maintenance_loop(
        owner,
        controller,
        app_core,
        500,
        "Ceremony acceptance paused; refresh to resume",
        WebUiOperation::ProcessCeremonyAcceptances,
        "WEB_CEREMONY_ACCEPTANCE_SLEEP_FAILED",
        move || {
            let agent = agent.clone();
            async move {
                if let Err(error) = agent.process_ceremony_acceptances().await {
                    log_web_error(
                        "warn",
                        &WebUiError::operation(
                            WebUiOperation::ProcessCeremonyAcceptances,
                            "WEB_CEREMONY_ACCEPTANCE_PROCESS_FAILED",
                            format!("{error}"),
                        ),
                    );
                }
            }
        },
    );
}

pub(crate) fn spawn_generation_maintenance_loops(
    generation_id: u64,
    controller: Arc<UiController>,
    app_core: Arc<RwLock<AppCore>>,
    account_ready: bool,
    agent: Option<Arc<AuraAgent>>,
) {
    let owner = replace_generation_maintenance_owner();
    web_sys::console::log_1(
        &format!(
            "[web-maintenance] generation={generation_id};account_ready={account_ready};has_agent={}",
            agent.is_some()
        )
        .into(),
    );
    if account_ready {
        spawn_background_sync_loop(&owner, controller.clone(), app_core.clone());
    }
    if let Some(agent) = agent {
        spawn_ceremony_acceptance_loop(&owner, controller, app_core, agent);
    }
}
