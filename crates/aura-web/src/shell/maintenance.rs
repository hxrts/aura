use async_lock::RwLock;
use aura_agent::AuraAgent;
use aura_app::ui::workflows::network as network_workflows;
use aura_app::ui::workflows::runtime as runtime_workflows;
use aura_app::ui::workflows::system as system_workflows;
use aura_app::AppCore;
use aura_ui::{FrontendUiOperation as WebUiOperation, UiController};
use base64::{engine::general_purpose::STANDARD, Engine};
use futures::channel::oneshot;
use gloo_net::http::Request;
use serde::Deserialize;
use std::cell::RefCell;
use std::future::Future;
use std::sync::Arc;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;

use crate::error::{log_web_error, WebUiError};
use crate::task_owner::{new_web_task_owner, WebTaskOwner};

const HARNESS_TRANSPORT_POLL_INTERVAL_MS: u64 = 100;
const HARNESS_TRANSPORT_POLL_PATH: &str = "/__aura_harness_transport__/poll";

#[derive(Clone)]
struct GenerationMaintenanceOwner {
    owner: WebTaskOwner,
}

#[derive(Clone)]
struct HarnessTransportTickContext {
    app_core: Arc<RwLock<AppCore>>,
    agent: Arc<AuraAgent>,
}

thread_local! {
    static ACTIVE_GENERATION_MAINTENANCE: RefCell<Option<GenerationMaintenanceOwner>> = const { RefCell::new(None) };
    static ACTIVE_HARNESS_TRANSPORT_TICK: RefCell<Option<HarnessTransportTickContext>> = const { RefCell::new(None) };
}

#[derive(Deserialize)]
struct HarnessTransportPollResponse {
    envelopes: Vec<String>,
}

#[cfg(target_arch = "wasm32")]
fn emit_browser_harness_debug_event(event: &str, detail: &str) {
    let Some(window) = web_sys::window() else {
        return;
    };
    let Ok(origin) = window.location().origin() else {
        return;
    };
    let event = js_sys::encode_uri_component(event)
        .as_string()
        .unwrap_or_else(|| event.to_string());
    let detail = js_sys::encode_uri_component(detail)
        .as_string()
        .unwrap_or_else(|| detail.to_string());
    let url = format!("{origin}/__aura_harness_debug__/event?event={event}&detail={detail}");
    let _ = window.fetch_with_str(&url);
}

#[cfg(not(target_arch = "wasm32"))]
fn emit_browser_harness_debug_event(_event: &str, _detail: &str) {}

fn harness_transport_poll_url(agent: &AuraAgent) -> Option<String> {
    let window = web_sys::window()?;
    let location = window.location();
    let origin = location.origin().ok()?;
    let authority = agent.authority_id().to_string();
    let device = agent.runtime().device_id().to_string();
    Some(format!(
        "{origin}{HARNESS_TRANSPORT_POLL_PATH}?authority={authority}&device={device}"
    ))
}

async fn drain_harness_transport_mailbox(agent: &AuraAgent) -> Result<usize, WebUiError> {
    let Some(url) = harness_transport_poll_url(agent) else {
        return Ok(0);
    };

    let response = Request::get(&url).send().await.map_err(|error| {
        WebUiError::operation(
            WebUiOperation::BackgroundSync,
            "WEB_HARNESS_TRANSPORT_POLL_FAILED",
            format!("failed to poll harness transport mailbox: {error}"),
        )
    })?;
    let payload = response
        .json::<HarnessTransportPollResponse>()
        .await
        .map_err(|error| {
            WebUiError::operation(
                WebUiOperation::BackgroundSync,
                "WEB_HARNESS_TRANSPORT_POLL_DECODE_FAILED",
                format!("failed to decode harness transport mailbox response: {error}"),
            )
        })?;

    let mut drained = 0_usize;
    for encoded in payload.envelopes {
        let bytes = STANDARD.decode(encoded.as_bytes()).map_err(|error| {
            WebUiError::operation(
                WebUiOperation::BackgroundSync,
                "WEB_HARNESS_TRANSPORT_BASE64_DECODE_FAILED",
                format!("failed to decode harness transport envelope bytes: {error}"),
            )
        })?;
        let envelope: aura_core::effects::transport::TransportEnvelope =
            aura_core::util::serialization::from_slice(&bytes).map_err(|error| {
                WebUiError::operation(
                    WebUiOperation::BackgroundSync,
                    "WEB_HARNESS_TRANSPORT_ENVELOPE_DECODE_FAILED",
                    format!("failed to decode harness transport envelope: {error}"),
                )
            })?;
        if let Some(content_type) = envelope.metadata.get("content-type") {
            if content_type == "application/aura-invitation"
                || content_type == "application/aura-invitation-acceptance+json"
            {
                web_sys::console::log_1(
                    &format!(
                        "[web-harness-transport] mailbox_drain authority={} context={} content_type={}",
                        envelope.destination, envelope.context, content_type
                    )
                    .into(),
                );
            }
        }
        agent.runtime().effects().requeue_envelope(envelope);
        drained = drained.saturating_add(1);
    }

    Ok(drained)
}

async fn browser_sleep_ms(ms: u64) -> Result<(), WebUiError> {
    let window = web_sys::window().ok_or_else(|| {
        WebUiError::operation(
            WebUiOperation::BackgroundSync,
            "WEB_BROWSER_SLEEP_WINDOW_UNAVAILABLE",
            "window unavailable for browser maintenance timer".to_string(),
        )
    })?;
    let timeout_ms = ms.min(i32::MAX as u64) as i32;
    let (tx, rx) = oneshot::channel::<()>();
    let callback = Closure::once(move || {
        let _ = tx.send(());
    });
    window
        .set_timeout_with_callback_and_timeout_and_arguments_0(
            callback.as_ref().unchecked_ref(),
            timeout_ms,
        )
        .map_err(|error| {
            WebUiError::operation(
                WebUiOperation::BackgroundSync,
                "WEB_BROWSER_SLEEP_SCHEDULE_FAILED",
                format!("failed to schedule browser maintenance timer: {error:?}"),
            )
        })?;
    callback.forget();
    rx.await.map_err(|_| {
        WebUiError::operation(
            WebUiOperation::BackgroundSync,
            "WEB_BROWSER_SLEEP_DROPPED",
            "browser maintenance timer dropped before completion".to_string(),
        )
    })?;
    Ok(())
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
    ACTIVE_HARNESS_TRANSPORT_TICK.with(|slot| {
        slot.borrow_mut().take();
    });
}

pub(crate) fn spawn_browser_maintenance_loop<F, Fut>(
    owner: &WebTaskOwner,
    _controller: Arc<UiController>,
    _app_core: Arc<RwLock<AppCore>>,
    interval_ms: u64,
    _pause_message: &'static str,
    _sleep_operation: WebUiOperation,
    _sleep_error_code: &'static str,
    mut tick: F,
) where
    F: FnMut() -> Fut + 'static,
    Fut: Future<Output = ()> + 'static,
{
    owner.spawn_local_cancellable(async move {
        loop {
            tick().await;
            if let Err(error) = browser_sleep_ms(interval_ms).await {
                log_web_error(
                    "warn",
                    &WebUiError::operation(_sleep_operation, _sleep_error_code, error.to_string()),
                );
                _controller.runtime_error_toast(_pause_message);
                break;
            }
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
        1_000,
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

fn spawn_harness_transport_poll_loop(
    owner: &WebTaskOwner,
    controller: Arc<UiController>,
    app_core: Arc<RwLock<AppCore>>,
    agent: Arc<AuraAgent>,
) {
    let tick_app_core = app_core.clone();
    spawn_browser_maintenance_loop(
        owner,
        controller,
        app_core,
        HARNESS_TRANSPORT_POLL_INTERVAL_MS,
        "Harness browser transport paused; refresh to resume",
        WebUiOperation::BackgroundSync,
        "WEB_HARNESS_TRANSPORT_SLEEP_FAILED",
        move || {
            let tick_app_core = tick_app_core.clone();
            let agent = agent.clone();
            async move {
                run_harness_transport_tick(tick_app_core, agent).await;
            }
        },
    );
}

async fn run_harness_transport_tick(app_core: Arc<RwLock<AppCore>>, agent: Arc<AuraAgent>) {
    emit_browser_harness_debug_event(
        "transport_tick_start",
        &format!("authority={}", agent.authority_id()),
    );
    let drained = match drain_harness_transport_mailbox(agent.as_ref()).await {
        Ok(drained) => drained,
        Err(error) => {
            emit_browser_harness_debug_event(
                "transport_tick_poll_error",
                &format!("authority={};error={}", agent.authority_id(), error),
            );
            log_web_error("warn", &error);
            return;
        }
    };
    emit_browser_harness_debug_event(
        "transport_tick_polled",
        &format!("authority={};drained={drained}", agent.authority_id()),
    );
    let _ = app_core;
    if drained == 0 {
        return;
    }
}

pub(crate) async fn run_harness_transport_tick_once() {
    let context = ACTIVE_HARNESS_TRANSPORT_TICK.with(|slot| slot.borrow().clone());
    if let Some(context) = context {
        run_harness_transport_tick(context.app_core, context.agent).await;
    }
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
        ACTIVE_HARNESS_TRANSPORT_TICK.with(|slot| {
            slot.borrow_mut().replace(HarnessTransportTickContext {
                app_core: app_core.clone(),
                agent: agent.clone(),
            });
        });
        spawn_harness_transport_poll_loop(
            &owner,
            controller.clone(),
            app_core.clone(),
            agent.clone(),
        );
        spawn_ceremony_acceptance_loop(&owner, controller, app_core, agent);
    } else if !account_ready {
        ACTIVE_HARNESS_TRANSPORT_TICK.with(|slot| {
            slot.borrow_mut().take();
        });
    }
}
