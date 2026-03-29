use async_lock::RwLock;
use aura_agent::AuraAgent;
use aura_app::frontend_primitives::FrontendUiOperation as WebUiOperation;
use aura_app::ui::workflows::network as network_workflows;
use aura_app::ui::workflows::runtime as runtime_workflows;
use aura_app::ui::workflows::system as system_workflows;
use aura_app::AppCore;
use aura_ui::UiController;
use base64::{engine::general_purpose::STANDARD, Engine};
use serde::Deserialize;
use std::cell::RefCell;
use std::future::Future;
use std::sync::Arc;

use crate::browser_promises::{browser_sleep_ms, fetch_text_with_timeout};
use crate::error::{log_web_error, WebUiError};
use crate::task_owner::{new_web_task_owner, WebTaskOwner};

// Browser-hosted harness transport polling must stay responsive without
// monopolizing the renderer main thread that Playwright relies on for page
// execution, selectors, and key dispatch. A 100 ms steady-state poll cadence
// is unnecessarily aggressive for preserved-profile browser scenarios and can
// starve interactive action channels. Keep the browser poll budget coarse by
// default; semantic queues and maintenance wakes still provide timely progress
// for real work.
const HARNESS_TRANSPORT_POLL_INTERVAL_MS: u64 = 1_000;
// Harness-mode browser scenarios need the page execution channel to stay
// available for semantic queue dispatch immediately after bootstrap handoff.
// Running the full browser background-sync pass every second from tick one can
// monopolize the renderer and starve Playwright page.evaluate / selector work
// even though semantic publication remains healthy. Keep the harness lane on a
// slower cadence with an initial grace window; production behavior stays
// unchanged.
const HARNESS_BACKGROUND_SYNC_INTERVAL_MS: u64 = 10_000;
const HARNESS_BACKGROUND_SYNC_START_DELAY_MS: u64 = 15_000;
const HARNESS_TRANSPORT_POLL_PATH: &str = "/__aura_harness_transport__/poll";

#[derive(Clone)]
struct GenerationMaintenanceOwner {
    owner: WebTaskOwner,
}

thread_local! {
    static ACTIVE_GENERATION_MAINTENANCE: RefCell<Option<GenerationMaintenanceOwner>> = const { RefCell::new(None) };
}

#[derive(Deserialize)]
struct HarnessTransportPollResponse {
    envelopes: Vec<String>,
}

#[cfg(target_arch = "wasm32")]
fn emit_browser_harness_debug_event(event: &str, detail: &str) {
    // Retained as an explicit no-op. Fetch-based per-tick debug emission from
    // browser maintenance creates additional browser-owned work on the same
    // main thread that powers Playwright actionability and page execution.
    // For preserved-profile harness restarts, that diagnostic traffic can
    // overwhelm the browser and make semantic action channels appear dead even
    // though publication remains healthy.
    let _ = (event, detail);
}

#[cfg(not(target_arch = "wasm32"))]
fn emit_browser_harness_debug_event(_event: &str, _detail: &str) {}

fn browser_harness_mode_enabled() -> bool {
    std::env::var_os("AURA_HARNESS_MODE").is_some()
}

fn interval_ticks(interval_ms: u64) -> u64 {
    interval_ms
        .div_ceil(HARNESS_TRANSPORT_POLL_INTERVAL_MS)
        .max(1)
}

fn delay_ticks(delay_ms: u64) -> u64 {
    delay_ms.div_ceil(HARNESS_TRANSPORT_POLL_INTERVAL_MS)
}

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

async fn yield_browser_maintenance_step(
    operation: WebUiOperation,
    error_code: &'static str,
    label: &'static str,
) -> Result<(), WebUiError> {
    browser_sleep_ms(
        0,
        operation,
        "WEB_BROWSER_SLEEP_WINDOW_UNAVAILABLE",
        error_code,
        "WEB_BROWSER_SLEEP_DROPPED",
        "window unavailable for browser maintenance yield",
        label,
    )
    .await
    .map_err(|error| WebUiError::operation(operation, error_code, error.to_string()))
}

async fn drain_harness_transport_mailbox(agent: &AuraAgent) -> Result<usize, WebUiError> {
    let Some(url) = harness_transport_poll_url(agent) else {
        return Ok(0);
    };
    let authority = agent.authority_id().to_string();
    emit_browser_harness_debug_event(
        "transport_poll_url_ready",
        &format!("authority={authority};url={url}"),
    );

    emit_browser_harness_debug_event(
        "transport_poll_fetch_begin",
        &format!("authority={authority}"),
    );
    let payload_text = fetch_text_with_timeout(
        &url,
        3_000,
        WebUiOperation::BackgroundSync,
        "WEB_HARNESS_TRANSPORT_POLL_FAILED",
        "WEB_HARNESS_TRANSPORT_POLL_TIMEOUT",
        "WEB_HARNESS_TRANSPORT_POLL_TEXT_AWAIT_FAILED",
        "WEB_HARNESS_TRANSPORT_POLL_TEXT_TIMEOUT",
    )
    .await?;
    emit_browser_harness_debug_event(
        "transport_poll_fetch_resolved",
        &format!("authority={authority}"),
    );
    emit_browser_harness_debug_event(
        "transport_poll_text_begin",
        &format!("authority={authority}"),
    );
    emit_browser_harness_debug_event(
        "transport_poll_text_resolved",
        &format!("authority={authority};bytes={}", payload_text.len()),
    );
    let payload =
        serde_json::from_str::<HarnessTransportPollResponse>(&payload_text).map_err(|error| {
            WebUiError::operation(
                WebUiOperation::BackgroundSync,
                "WEB_HARNESS_TRANSPORT_POLL_DECODE_FAILED",
                format!("failed to decode harness transport mailbox response: {error}"),
            )
        })?;
    emit_browser_harness_debug_event(
        "transport_poll_decode_done",
        &format!(
            "authority={authority};envelopes={}",
            payload.envelopes.len()
        ),
    );

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
            if let Err(error) = browser_sleep_ms(
                interval_ms,
                _sleep_operation,
                "WEB_BROWSER_SLEEP_WINDOW_UNAVAILABLE",
                _sleep_error_code,
                "WEB_BROWSER_SLEEP_DROPPED",
                "window unavailable for browser maintenance timer",
                "browser maintenance timer",
            )
            .await
            {
                log_web_error(
                    "warn",
                    &WebUiError::operation(_sleep_operation, _sleep_error_code, error.to_string()),
                );
                _controller.runtime_error_toast(_pause_message);
                break;
            }
            tick().await;
        }
    });
}

async fn run_background_sync_pass(app_core: Arc<RwLock<AppCore>>) {
    let runtime = { app_core.read().await.runtime().cloned() };
    if let Some(runtime) = runtime {
        if let Err(error) = yield_browser_maintenance_step(
            WebUiOperation::BackgroundSync,
            "WEB_BACKGROUND_SYNC_YIELD_FAILED",
            "background sync before trigger_discovery",
        )
        .await
        {
            log_web_error("warn", &error);
            return;
        }
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
        if let Err(error) = yield_browser_maintenance_step(
            WebUiOperation::BackgroundSync,
            "WEB_BACKGROUND_SYNC_YIELD_FAILED",
            "background sync before process_ceremony_messages_before_sync",
        )
        .await
        {
            log_web_error("warn", &error);
            return;
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
        if let Err(error) = yield_browser_maintenance_step(
            WebUiOperation::BackgroundSync,
            "WEB_BACKGROUND_SYNC_YIELD_FAILED",
            "background sync before trigger_sync",
        )
        .await
        {
            log_web_error("warn", &error);
            return;
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
        if let Err(error) = yield_browser_maintenance_step(
            WebUiOperation::BackgroundSync,
            "WEB_BACKGROUND_SYNC_YIELD_FAILED",
            "background sync before process_ceremony_messages_after_sync",
        )
        .await
        {
            log_web_error("warn", &error);
            return;
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
    if let Err(error) = yield_browser_maintenance_step(
        WebUiOperation::BackgroundSync,
        "WEB_BACKGROUND_SYNC_YIELD_FAILED",
        "background sync before refresh_account",
    )
    .await
    {
        log_web_error("warn", &error);
        return;
    }
    if let Err(error) = system_workflows::refresh_account(&app_core).await {
        log_web_error(
            "warn",
            &WebUiError::operation(
                WebUiOperation::BackgroundSync,
                "WEB_REFRESH_ACCOUNT_FAILED",
                error.to_string(),
            ),
        );
    }
    if let Err(error) = yield_browser_maintenance_step(
        WebUiOperation::BackgroundSync,
        "WEB_BACKGROUND_SYNC_YIELD_FAILED",
        "background sync before refresh_discovered_peers",
    )
    .await
    {
        log_web_error("warn", &error);
        return;
    }
    if let Err(error) = network_workflows::refresh_discovered_peers(&app_core).await {
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

async fn run_ceremony_acceptance_pass(agent: Arc<AuraAgent>) {
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

fn spawn_generation_maintenance_supervisor(
    owner: &WebTaskOwner,
    controller: Arc<UiController>,
    app_core: Arc<RwLock<AppCore>>,
    account_ready: bool,
    agent: Option<Arc<AuraAgent>>,
) {
    let tick_app_core = app_core.clone();
    let ceremony_interval_ticks = interval_ticks(500);
    let background_sync_interval_ticks = if browser_harness_mode_enabled() {
        interval_ticks(HARNESS_BACKGROUND_SYNC_INTERVAL_MS)
    } else {
        interval_ticks(1_000)
    };
    let background_sync_start_delay_ticks = if browser_harness_mode_enabled() {
        delay_ticks(HARNESS_BACKGROUND_SYNC_START_DELAY_MS)
    } else {
        0
    };
    let mut tick_count: u64 = 0;
    spawn_browser_maintenance_loop(
        owner,
        controller,
        app_core,
        HARNESS_TRANSPORT_POLL_INTERVAL_MS,
        "Browser maintenance paused; refresh to resume",
        WebUiOperation::BackgroundSync,
        "WEB_GENERATION_MAINTENANCE_SLEEP_FAILED",
        move || {
            tick_count = tick_count.saturating_add(1);
            let tick_app_core = tick_app_core.clone();
            let agent = agent.clone();
            let run_ceremony = agent.is_some() && tick_count % ceremony_interval_ticks == 0;
            let run_background_sync = account_ready
                && tick_count >= background_sync_start_delay_ticks
                && tick_count % background_sync_interval_ticks == 0;
            async move {
                if let Some(agent) = agent.clone() {
                    run_harness_transport_tick(tick_app_core.clone(), agent.clone()).await;
                    if run_ceremony {
                        run_ceremony_acceptance_pass(agent).await;
                    }
                }
                if run_background_sync {
                    run_background_sync_pass(tick_app_core).await;
                }
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
    if drained == 0 {
        emit_browser_harness_debug_event(
            "transport_tick_done",
            &format!("authority={};drained=0", agent.authority_id()),
        );
        return;
    }

    let runtime = { app_core.read().await.runtime().cloned() };
    if let Some(runtime) = runtime {
        if let Err(error) = yield_browser_maintenance_step(
            WebUiOperation::BackgroundSync,
            "WEB_HARNESS_TRANSPORT_YIELD_FAILED",
            "harness transport before maintenance pass",
        )
        .await
        {
            emit_browser_harness_debug_event(
                "transport_tick_maintenance_yield_error",
                &format!("authority={};error={error}", agent.authority_id()),
            );
            log_web_error("warn", &error);
            return;
        }
        emit_browser_harness_debug_event(
            "transport_tick_maintenance_start",
            &format!("authority={};drained={drained}", agent.authority_id()),
        );
        if let Err(error) =
            runtime_workflows::run_harness_runtime_mailbox_pass(&app_core, &runtime).await
        {
            emit_browser_harness_debug_event(
                "transport_tick_maintenance_error",
                &format!("authority={};error={error}", agent.authority_id()),
            );
            log_web_error(
                "warn",
                &WebUiError::operation(
                    WebUiOperation::BackgroundSync,
                    "WEB_HARNESS_TRANSPORT_MAINTENANCE_FAILED",
                    error.to_string(),
                ),
            );
        } else {
            emit_browser_harness_debug_event(
                "transport_tick_maintenance_done",
                &format!("authority={};drained={drained}", agent.authority_id()),
            );
        }
    } else {
        emit_browser_harness_debug_event(
            "transport_tick_runtime_missing",
            &format!("authority={};drained={drained}", agent.authority_id()),
        );
    }
    emit_browser_harness_debug_event(
        "transport_tick_done",
        &format!("authority={};drained={drained}", agent.authority_id()),
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
    spawn_generation_maintenance_supervisor(&owner, controller, app_core, account_ready, agent);
}
